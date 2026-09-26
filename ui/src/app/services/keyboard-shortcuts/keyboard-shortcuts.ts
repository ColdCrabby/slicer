import { Injectable, Injector, inject } from '@angular/core';
import { Router } from '@angular/router';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { fromEvent } from 'rxjs';
import { filter, map } from 'rxjs/operators';
import { matchKeybindingPress, parseKeybinding } from 'tinykeys';
import { Arrange } from '../arrange';
import { LibraryFlyout } from '../library/library-flyout';
import type { GcodePreview } from '../gcode-preview';
import { SceneEngine } from '../scene-engine';
import { SceneHistory } from '../scene-history/scene-history';
import { Slicer } from '../slicer';
import { SceneCommand } from '../scene-command/scene-command';
import { type ObjectMode, ViewerControl } from '../viewer-control';
import { WorkplateObjects } from '../workplate-objects/workplate-objects';
import { nudgeDelta, type NudgeKey } from './nudge';
import {
  isApplePlatform,
  isTauriHost,
  isTauriMobile,
} from '../../runtime/domain/runtime-mode.util';

export interface ShortcutConfig {
  actionId: string;
  /** tinykeys-format shortcut string, e.g. `"$mod+z"`, `"$mod+Shift+z"`, `"a"`. */
  shortcut: string;
  /** Human-readable description of what the action does. */
  displayDescription: string;
  canMatch?: () => boolean;
  /** Receives the key press, for actions that scale with a held modifier. */
  handleAction: (event: KeyboardEvent) => void;
  /**
   * `false` keeps a binding out of Settings → Shortcuts — for a variant that
   * the listed entry already describes, such as a nudge's fine step.
   */
  listed?: boolean;
}

type ParsedShortcutConfig = ShortcutConfig & {
  _parsed: ReturnType<typeof parseKeybinding>;
};

/**
 * Registers global keyboard shortcuts for undo/redo and scene operations.
 *
 * Must be eagerly instantiated — inject this class in the root `App`
 * component constructor to ensure shortcuts are active for the entire
 * application lifetime.
 */
@Injectable({ providedIn: 'root' })
export class KeyboardShortcuts {
  private readonly history = inject(SceneHistory);
  private readonly arrange = inject(Arrange);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly viewerControl = inject(ViewerControl);
  private readonly slicer = inject(Slicer);
  private readonly injector = inject(Injector);
  private readonly workplate = inject(WorkplateObjects);
  private readonly sceneCommand = inject(SceneCommand);
  private readonly libraryFlyout = inject(LibraryFlyout);
  private readonly router = inject(Router);

  /**
   * True when running on macOS desktop/laptop (not iPadOS). Consumers use
   * this to decide which viewport-navigation model applies — kept in sync
   * with the trackpad gesture branch in {@link SceneControls}. iPadOS is
   * excluded because it uses the touch pointer path, not the trackpad
   * wheel path.
   */
  readonly isMac = detectMac();

  private readonly shortcuts: ParsedShortcutConfig[] = [
    {
      actionId: 'undo',
      shortcut: '$mod+z',
      displayDescription: 'Undo',
      // Gated like every other shortcut here: without this, correcting a typo
      // in a settings field reached past the caret and undid the last *scene*
      // operation instead.
      canMatch: () => !this.isTextInputFocused() && this.history.canUndo(),
      handleAction: () => this.history.undo(),
    },
    {
      actionId: 'redo',
      shortcut: '$mod+y',
      displayDescription: 'Redo',
      canMatch: () => !this.isTextInputFocused() && this.history.canRedo(),
      handleAction: () => this.history.redo(),
    },
    {
      actionId: 'redo-alt',
      shortcut: '$mod+Shift+z',
      displayDescription: 'Redo (alternate)',
      canMatch: () => !this.isTextInputFocused() && this.history.canRedo(),
      handleAction: () => this.history.redo(),
    },
    {
      actionId: 'place-objects',
      shortcut: 'a',
      displayDescription: 'Place objects on the bed',
      // Gated on model view like Select all: in G-code preview this would
      // move the plate behind a picture the user cannot see it change.
      canMatch: () =>
        !this.isTextInputFocused() &&
        this.viewerControl.viewMode() === 'model' &&
        this.sceneEngine.objects().length > 0,
      handleAction: () => this.arrange.run(),
    },
    {
      actionId: 'select-all',
      shortcut: '$mod+a',
      displayDescription: 'Select all objects',
      canMatch: () =>
        !this.isTextInputFocused() &&
        this.viewerControl.viewMode() === 'model' &&
        this.sceneEngine.objects().length > 0,
      // Writing the shared selection signal is enough: the viewer mirrors it
      // into the 3D scene, so this never needs a handle on the viewer.
      handleAction: () =>
        this.viewerControl.selectedObjectIds.set(this.sceneEngine.objects().map((o) => o.id)),
    },
    {
      actionId: 'remove-selected',
      shortcut: 'Delete',
      displayDescription: 'Remove the selected objects',
      canMatch: () => this.canEditSelection(),
      handleAction: () => this.removeSelected(),
    },
    {
      actionId: 'remove-selected-alt',
      shortcut: 'Backspace',
      displayDescription: 'Remove the selected objects (alternate)',
      canMatch: () => this.canEditSelection(),
      handleAction: () => this.removeSelected(),
    },
    {
      actionId: 'duplicate-selected',
      shortcut: '$mod+d',
      displayDescription: 'Duplicate the selected objects',
      canMatch: () => this.canEditSelection(),
      // The copies become the selection, so pressing it again stamps another
      // row, and the next drag moves the copies rather than the originals.
      handleAction: () =>
        this.viewerControl.selectedObjectIds.set(
          this.workplate.duplicateAll(this.viewerControl.selectedObjectIds()),
        ),
    },
    {
      actionId: 'slice',
      shortcut: '$mod+Enter',
      displayDescription: 'Slice the plate',
      canMatch: () =>
        sceneHost() !== null &&
        this.sceneEngine.objects().length > 0 &&
        this.slicer.status() !== 'slicing' &&
        this.slicer.status() !== 'uploading',
      handleAction: () => void this.slicer.slice(),
    },
    {
      // First of the Escapes: the library is laid over everything else on the
      // plate, so it is what the first press puts away. From its own search
      // field too — that field empties itself on Escape first, and only an
      // empty one lets the key through to here.
      actionId: 'library-close',
      shortcut: 'Escape',
      displayDescription: 'Close the library',
      canMatch: () =>
        this.libraryFlyout.open() &&
        (!this.isTextInputFocused() || this.focusIsIn('.library-flyout')),
      handleAction: () => this.libraryFlyout.close(),
    },
    {
      actionId: 'library-deselect',
      shortcut: 'Escape',
      displayDescription: 'Library: clear the selected model',
      canMatch: () => !this.isTextInputFocused() && !!this.libraryPageRef?.hasSelection(),
      handleAction: () => this.libraryPageRef!.deselect(),
    },
    {
      // Before `deselect-all`, which shares the key: the first press should peel
      // the keyboard off the card, not act on the plate behind it. Tab goes in,
      // Escape comes out — Shift+Tab would work too, but only after walking back
      // through whatever header actions the card puts before its fields.
      actionId: 'leave-tool-panel',
      shortcut: 'Escape',
      displayDescription: 'Leave the tool panel, back to the plate',
      canMatch: () => document.activeElement?.closest('.tool-dock') != null,
      handleAction: () => sceneHost()?.focus({ preventScroll: true }),
    },
    {
      // Before `deselect-all`: a painting or face-picking tool is a mode the
      // user is *in*, and Escape is how every app lets you out of one. The
      // plain transform tools only give way once the selection is already
      // clear, so the first Escape there still deselects.
      actionId: 'leave-tool',
      shortcut: 'Escape',
      displayDescription: 'Put the tool down, back to Select & move',
      canMatch: () => {
        const mode = this.viewerControl.objectMode();
        const modal = mode === 'paint' || mode === 'pullToFloor' || mode === 'place';
        return (
          this.onPlate() &&
          mode !== 'translate' &&
          this.viewerControl.brushPopoutAt() === null &&
          (modal || this.viewerControl.selectedObjectIds().length === 0)
        );
      },
      handleAction: () => this.viewerControl.objectMode.set('translate'),
    },
    {
      actionId: 'deselect-all',
      shortcut: 'Escape',
      displayDescription: 'Clear the selection',
      canMatch: () =>
        !this.isTextInputFocused() && this.viewerControl.selectedObjectIds().length > 0,
      handleAction: () => this.viewerControl.selectedObjectIds.set([]),
    },
    {
      actionId: 'object-mode-translate',
      shortcut: 'm',
      displayDescription: 'Select & move tool',
      canMatch: () => this.onPlate(),
      handleAction: () => this.viewerControl.objectMode.set('translate'),
    },
    {
      actionId: 'object-mode-rotate',
      shortcut: 'r',
      displayDescription: 'Rotate tool (press again to put it down)',
      canMatch: () => this.onPlate(),
      handleAction: () => this.toggleTool('rotate'),
    },
    {
      actionId: 'object-mode-scale',
      shortcut: 's',
      displayDescription: 'Scale tool (press again to put it down)',
      canMatch: () => this.onPlate(),
      handleAction: () => this.toggleTool('scale'),
    },
    {
      actionId: 'object-mode-pull-to-floor',
      shortcut: 'f',
      displayDescription: 'Pull a face to the floor (press again to put it down)',
      canMatch: () => this.onPlate(),
      handleAction: () => this.toggleTool('pullToFloor'),
    },
    {
      actionId: 'object-mode-paint',
      shortcut: 'b',
      displayDescription: 'Paint supports (press again to put the brush down)',
      canMatch: () => this.onPlate(),
      handleAction: () =>
        this.viewerControl.objectMode() === 'paint' && this.viewerControl.viewMode() === 'model'
          ? this.viewerControl.objectMode.set('translate')
          : this.enterPaintMode(),
    },
    {
      actionId: 'brush-quick-adjust',
      shortcut: 'Shift+b',
      displayDescription: 'Brush size and mode, at the pointer',
      canMatch: () => this.onPlate(),
      handleAction: () => this.toggleBrushPopout(),
    },
    {
      actionId: 'toggle-gravity',
      shortcut: 'g',
      displayDescription: 'Toggle gravity',
      canMatch: () => this.onPlate(),
      handleAction: () => this.viewerControl.gravityEnabled.update((v) => !v),
    },
    {
      actionId: 'toggle-view-mode',
      shortcut: 'p',
      displayDescription: 'Toggle G-code preview / model view',
      canMatch: () => this.onPlate(),
      handleAction: () => this.toggleViewMode(),
    },
    {
      actionId: 'toggle-projection',
      shortcut: 'Shift+Space',
      displayDescription: 'Switch between orthographic and perspective views',
      canMatch: () => this.onPlate(),
      handleAction: () => this.toggleProjection(),
    },
    {
      actionId: 'gcode-next-extrusion',
      shortcut: 'ArrowRight',
      displayDescription: 'Next extrusion (G-code viewer)',
      canMatch: () => this.onPlate() && this.viewerControl.viewMode() === 'gcode',
      handleAction: () => this.gcodeNextExtrusion(),
    },
    {
      actionId: 'gcode-prev-extrusion',
      shortcut: 'ArrowLeft',
      displayDescription: 'Previous extrusion (G-code viewer)',
      canMatch: () => this.onPlate() && this.viewerControl.viewMode() === 'gcode',
      handleAction: () => this.gcodePrevExtrusion(),
    },
    {
      actionId: 'gcode-next-layer',
      shortcut: 'ArrowUp',
      displayDescription: 'Next layer (G-code viewer)',
      canMatch: () => this.onPlate() && this.viewerControl.viewMode() === 'gcode',
      handleAction: () => this.gcodeNextLayer(),
    },
    {
      actionId: 'gcode-prev-layer',
      shortcut: 'ArrowDown',
      displayDescription: 'Previous layer (G-code viewer)',
      canMatch: () => this.onPlate() && this.viewerControl.viewMode() === 'gcode',
      handleAction: () => this.gcodePrevLayer(),
    },
    ...this.nudgeShortcuts(),
    {
      actionId: 'zoom-to-selection',
      shortcut: 'z',
      displayDescription: 'Zoom to the selection, or to everything',
      canMatch: () => this.onPlate() && this.sceneEngine.objects().length > 0,
      handleAction: () => this.viewerControl.frameObjects(this.viewerControl.selectedObjectIds()),
    },
    {
      actionId: 'focus-tool-panel',
      shortcut: 'Tab',
      displayDescription: 'Jump into the active tool panel',
      // Only from the scene, and only when there is a card to jump into, so
      // every other Tab in the app stays the browser's own.
      canMatch: () => this.focusIsOnTheScene() && toolPanelTarget() !== null,
      handleAction: () => toolPanelTarget()?.focus(),
    },
    {
      actionId: 'toggle-library',
      shortcut: 'l',
      displayDescription: 'Open or close the library',
      canMatch: () => !this.isTextInputFocused() && !this.router.url.startsWith('/library'),
      // Over the plate when there is one, as the rail does; the page otherwise.
      handleAction: () =>
        sceneHost() !== null
          ? this.libraryFlyout.toggle()
          : void this.router.navigate(['/library']),
    },
    {
      actionId: 'toggle-print-settings',
      shortcut: '$mod+Backslash',
      displayDescription: 'Dock or hide the print settings',
      canMatch: () => this.printSettingsRef !== null,
      handleAction: () => this.printSettingsRef!.toggle(),
    },
    {
      // Before the settings search: with the library open over the plate, both
      // are mounted, and the library is the one on top.
      actionId: 'focus-library-search',
      shortcut: '$mod+f',
      displayDescription: 'Search the library',
      canMatch: () => this.librarySearchRef !== null,
      handleAction: () => this.librarySearchRef!.focusSearch(),
    },
    {
      actionId: 'library-add-models',
      shortcut: '$mod+o',
      displayDescription: 'Library: add models from files',
      canMatch: () => this.libraryPageRef !== null,
      handleAction: () => this.libraryPageRef!.addModels(),
    },
    {
      actionId: 'library-rename',
      shortcut: 'F2',
      displayDescription: 'Library: rename the selected model',
      canMatch: () => !this.isTextInputFocused() && !!this.libraryPageRef?.hasSelection(),
      handleAction: () => this.libraryPageRef!.rename(),
    },
    {
      actionId: 'library-remove',
      shortcut: 'Delete',
      displayDescription: 'Library: remove the selected model (press twice)',
      canMatch: () => !this.isTextInputFocused() && !!this.libraryPageRef?.hasSelection(),
      handleAction: () => this.libraryPageRef!.remove(),
    },
    {
      actionId: 'library-remove-alt',
      shortcut: 'Backspace',
      displayDescription: 'Library: remove the selected model (alternate)',
      canMatch: () => !this.isTextInputFocused() && !!this.libraryPageRef?.hasSelection(),
      handleAction: () => this.libraryPageRef!.remove(),
    },
    {
      actionId: 'focus-settings-search',
      shortcut: '$mod+f',
      displayDescription: 'Focus settings search',
      canMatch: () => this.settingsSearchRef !== null,
      handleAction: () => this.settingsSearchRef!.focusSearch(),
    },
    {
      actionId: 'search-tabs',
      shortcut: '$mod+Shift+a',
      displayDescription: 'Search open workplates',
      canMatch: () => this.tabStripRef !== null,
      handleAction: () => this.tabStripRef!.toggleSearch(),
    },
    ...this.workplateTabShortcuts(),
  ].map((s) => ({ ...s, _parsed: parseKeybinding(s.shortcut) }));

  /**
   * Whichever settings search is currently on screen — the slice sidebar's
   * schema form, or a profile editor's outline filter.
   *
   * Set on mount and cleared on destroy. The two are never mounted together
   * (one is the slice page, the other the settings pages), so a single slot
   * serves both and `$mod+f` means the same thing wherever the user is.
   */
  settingsSearchRef: { focusSearch(): void } | null = null;

  /** The titlebar's workplate tab strip, which every tab shortcut drives. */
  tabStripRef: WorkplateTabStrip | null = null;

  /** Whichever library grid is on screen — the page's, or the flyout's. */
  librarySearchRef: { focusSearch(): void } | null = null;

  /** The full library page, while it is the page on screen. */
  libraryPageRef: LibraryPageShortcuts | null = null;

  /** The slice view's print settings panel. */
  printSettingsRef: { toggle(): void } | null = null;

  constructor() {
    fromEvent<KeyboardEvent>(document, 'keydown')
      .pipe(
        map((event) => ({ event, shortcut: this.findMatch(event) })),
        filter(({ shortcut }) => shortcut !== null),
        takeUntilDestroyed(),
      )
      .subscribe(({ event, shortcut }) => {
        event.preventDefault();
        shortcut!.handleAction(event);
      });
  }

  /**
   * The browser's own tab keys, wherever the page can actually have them.
   *
   * In the desktop and iPad apps `$mod+W`, `$mod+T` and friends reach the
   * webview like any other key, so they mean what they mean in every tabbed
   * app. A browser keeps them for its own tabs — `Ctrl+W` closes the *browser*
   * tab and no page can stop it — so the web build answers the same actions on
   * `Alt` instead, and warns before the page is left (see `WorkplateTabs`).
   *
   * On the Mac desktop app the close/new/reopen keys belong to the native menu
   * bar (`ui-desktop/src-tauri/src/app_menu.rs`), which forwards them here as
   * events; answering the keydown too would run each of them twice.
   */
  private workplateTabShortcuts(): ShortcutConfig[] {
    const native = isTauriHost();
    const menuOwnsKeys = native && !isTauriMobile() && this.isMac;
    const strip = (): WorkplateTabStrip | null => this.tabStripRef;
    // The `Alt` fallbacks are letters a text field would otherwise type (⌥W is
    // "∑" on a Mac), so they stand down while typing; `$mod` chords never type.
    const whenFree = (): boolean => strip() !== null && (native || !this.isTextInputFocused());

    const configs: (ShortcutConfig | false)[] = [
      !menuOwnsKeys && {
        actionId: 'workplate-close',
        shortcut: native ? '$mod+w' : 'Alt+KeyW',
        displayDescription: 'Close workplate',
        canMatch: whenFree,
        handleAction: () => void strip()!.closeActive(),
      },
      !menuOwnsKeys && {
        actionId: 'workplate-close-all',
        shortcut: native ? '$mod+Shift+w' : 'Alt+Shift+KeyW',
        displayDescription: 'Close all workplates',
        canMatch: whenFree,
        handleAction: () => void strip()!.closeAll(),
      },
      !menuOwnsKeys && {
        actionId: 'workplate-new',
        shortcut: native ? '$mod+t' : 'Alt+KeyT',
        displayDescription: 'New workplate',
        canMatch: whenFree,
        handleAction: () => void strip()!.addTab(),
      },
      !menuOwnsKeys && {
        actionId: 'workplate-reopen',
        shortcut: native ? '$mod+Shift+t' : 'Alt+Shift+KeyT',
        displayDescription: 'Reopen closed workplate',
        canMatch: whenFree,
        handleAction: () => strip()!.reopenClosed(),
      },
      {
        actionId: 'workplate-next',
        shortcut: native ? 'Control+Tab' : 'Alt+Shift+ArrowRight',
        displayDescription: 'Next workplate',
        canMatch: whenFree,
        handleAction: () => strip()!.cycle(1),
      },
      {
        actionId: 'workplate-previous',
        shortcut: native ? 'Control+Shift+Tab' : 'Alt+Shift+ArrowLeft',
        displayDescription: 'Previous workplate',
        canMatch: whenFree,
        handleAction: () => strip()!.cycle(-1),
      },
    ];
    // `$mod+1`…`$mod+8` pick a tab by position and `$mod+9` the last, as in a
    // browser. Desktop only: the browser keeps these for its own tabs too.
    if (native) {
      for (let n = 1; n <= 9; n++) {
        configs.push({
          actionId: `workplate-select-${n}`,
          shortcut: `$mod+Digit${n}`,
          displayDescription: n === 9 ? 'Go to the last workplate' : `Go to workplate ${n}`,
          canMatch: whenFree,
          handleAction: () => strip()!.activateIndex(n === 9 ? -1 : n - 1),
        });
      }
    }
    return configs.filter((config): config is ShortcutConfig => config !== false);
  }

  /**
   * Arrow keys nudge the selection across the bed, as seen from the camera.
   *
   * Shift takes a coarse step and ⌥/Alt a fine one — the same ×10 / ×0.1 the
   * number fields use, so the hand learns one rule. Model view only: in G-code
   * preview the same keys walk layers and extrusions. Holding a key repeats,
   * and every repeat inside the history's pause lands in one undo step.
   */
  private nudgeShortcuts(): ShortcutConfig[] {
    const alt = isApplePlatform() ? '⌥' : 'Alt';
    const keys: [NudgeKey, string][] = [
      ['ArrowUp', 'away'],
      ['ArrowDown', 'towards you'],
      ['ArrowLeft', 'left'],
      ['ArrowRight', 'right'],
    ];
    return keys.flatMap(([key, towards]): ShortcutConfig[] => [
      {
        actionId: `nudge-${key}`,
        shortcut: `[Shift]+${key}`,
        displayDescription: `Nudge the selection ${towards} 1 mm (Shift 10 mm, ${alt} 0.1 mm)`,
        canMatch: () => this.canNudge(),
        handleAction: (event) => this.nudge(key, event.shiftKey ? 10 : 1),
      },
      {
        actionId: `nudge-fine-${key}`,
        shortcut: `Alt+${key}`,
        displayDescription: `Nudge the selection ${towards} 0.1 mm`,
        listed: false,
        canMatch: () => this.canNudge(),
        handleAction: () => this.nudge(key, 0.1),
      },
    ]);
  }

  /**
   * A selection to move, and no focused control that steers with the arrows
   * itself — the tool radio group, a slider, a menu — or one press would both
   * change the tool and shove the part.
   */
  private canNudge(): boolean {
    const active = document.activeElement;
    return (
      this.canEditSelection() &&
      !(active instanceof Element && active.closest(ARROW_KEY_WIDGETS) !== null)
    );
  }

  private nudge(key: NudgeKey, step: number): void {
    const { direction, up } = this.viewerControl.cameraState;
    const [dx, dy] = nudgeDelta(key, step, direction, up);
    for (const id of this.viewerControl.selectedObjectIds()) {
      this.sceneCommand.apply({ op: 'Translate', args: { id, delta: [dx, dy, 0] } });
    }
  }

  /** Pick up a tool, or put it down if it is already in hand. */
  private toggleTool(mode: ObjectMode): void {
    const control = this.viewerControl.objectMode;
    control.set(control() === mode ? 'translate' : mode);
  }

  /**
   * Returns a human-readable shortcut label for the given action ID,
   * or `'unset'` if no shortcut is registered.
   *
   * `$mod` is resolved to `Ctrl` on Windows/Linux and `⌘` on macOS.
   */
  shortcutFor(actionId: string): string {
    const config = this.shortcuts.find((s) => s.actionId === actionId);
    if (!config) {
      return 'unset';
    }
    const apple = isApplePlatform();
    return (
      config.shortcut
        .replace(/\$mod/g, apple ? '⌘' : 'Ctrl')
        // Physical-key names are how a binding survives ⌥ turning W into ∑;
        // nobody reads a key cap as "KeyW".
        .replace(/\b(?:Key|Digit)(\w)\b/g, '$1')
        .replace(/\bAlt\b/g, apple ? '⌥' : 'Alt')
        .replace(/\bControl\b/g, apple ? '⌃' : 'Ctrl')
        // An optional modifier (`[Shift]+`) is a variant the description
        // explains, not a key that has to be held.
        .replace(/\[\w+\]\+/g, '')
        .replace(/\bArrow(Up|Down|Left|Right)\b/g, (_, dir: string) => ARROW_GLYPHS[dir])
    );
  }

  /** Returns all registered shortcuts as plain data for display in a panel. */
  getAll(): { actionId: string; displayText: string; displayDescription: string }[] {
    return this.shortcuts
      .filter((s) => s.listed !== false)
      .map(({ actionId, displayDescription }) => ({
        actionId,
        displayText: this.shortcutFor(actionId),
        displayDescription,
      }));
  }

  private findMatch(event: KeyboardEvent): ShortcutConfig | null {
    return (
      this.shortcuts.find(
        (s) =>
          s._parsed.every((press) => matchKeybindingPress(event, press)) &&
          (s.canMatch?.() ?? true),
      ) ?? null
    );
  }

  private toggleViewMode(): void {
    if (this.viewerControl.viewMode() === 'gcode') {
      this.viewerControl.viewMode.set('model');
      return;
    }
    this.viewerControl.viewMode.set('gcode');
    this.withGcodePreview((preview) => {
      const status = this.slicer.status();
      if (!preview.gcodeHandle() && status !== 'slicing' && status !== 'uploading') {
        void this.slicer.slice();
      }
    });
  }

  private toggleProjection(): void {
    const currentView = this.viewerControl.view();
    const newView = currentView === 'perspective' ? 'ortho' : 'perspective';
    this.viewerControl.view.set(newView);
  }

  /**
   * Enter paint mode, leaving G-code preview if that is where we are.
   *
   * The paint tools only exist over the model, so pressing `b` in preview used
   * to appear to do nothing at all: the mode changed behind a view that cannot
   * show it. Reaching for the brush is a clear enough statement of intent to
   * switch back on the user's behalf.
   */
  private enterPaintMode(): void {
    this.viewerControl.viewMode.set('model');
    this.viewerControl.objectMode.set('paint');
  }

  /** Open (or dismiss) the quick-adjust brush card at the pointer. */
  private toggleBrushPopout(): void {
    if (this.viewerControl.brushPopoutAt() !== null) {
      this.viewerControl.brushPopoutAt.set(null);
      return;
    }
    this.enterPaintMode();
    const at = this.viewerControl.pointerPositionSource?.() ?? null;
    this.viewerControl.brushPopoutAt.set(
      at ?? { x: window.innerWidth / 2, y: window.innerHeight / 2 },
    );
  }

  /**
   * Whether the keyboard is "on the plate" rather than in a control.
   *
   * The viewer host carries `tabindex="0"` so the scene can hold focus; `body`
   * counts too, for the moment before anything has been clicked.
   */
  private focusIsOnTheScene(): boolean {
    const active = document.activeElement;
    return (
      active === null ||
      active === document.body ||
      (active instanceof Element && active.classList.contains('viewer-host'))
    );
  }

  /**
   * The plate is on screen and the keyboard is not in a text field.
   *
   * Plate shortcuts are single keys; without the first half, pressing `p` on a
   * settings page started a slice behind it, and without the second the G-code
   * arrows stole the caret from the settings search.
   */
  private focusIsIn(selector: string): boolean {
    return document.activeElement?.closest(selector) != null;
  }

  private onPlate(): boolean {
    return sceneHost() !== null && !this.isTextInputFocused();
  }

  /** A selection exists on the plate in model view and can be edited. */
  private canEditSelection(): boolean {
    return (
      this.onPlate() &&
      this.viewerControl.viewMode() === 'model' &&
      this.viewerControl.selectedObjectIds().length > 0
    );
  }

  /** Remove every selected object; one undo brings them all back. */
  private removeSelected(): void {
    this.workplate.removeAll(this.viewerControl.selectedObjectIds());
    this.viewerControl.selectedObjectIds.set([]);
  }

  private isTextInputFocused(): boolean {
    const target = document.activeElement as HTMLElement | null;
    if (!target) {
      return false;
    }
    const tag = target.tagName.toUpperCase();
    return (
      (tag === 'INPUT' && !NON_TYPING_INPUT_TYPES.has(inputTypeOf(target))) ||
      tag === 'TEXTAREA' ||
      tag === 'SELECT' ||
      target.isContentEditable ||
      // A control inside an open dialog owns the keyboard for as long as the
      // dialog is up; scene shortcuts firing behind it act on something the
      // user cannot even see.
      target.closest('dialog[open]') !== null
    );
  }

  private gcodeNextExtrusion(): void {
    this.withGcodePreview((preview) => preview.stepSegment(1));
  }

  private gcodePrevExtrusion(): void {
    this.withGcodePreview((preview) => preview.stepSegment(-1));
  }

  private gcodeNextLayer(): void {
    this.withGcodePreview((preview) => preview.stepLayer(1));
  }

  private gcodePrevLayer(): void {
    this.withGcodePreview((preview) => preview.stepLayer(-1));
  }

  /**
   * Reach the G-code preview without making this service depend on it.
   *
   * This service is constructed at startup, so a static import here would put
   * the preview — and the scene engine's wasm glue it imports — in the initial
   * bundle, and injecting it would start its effects before any plate exists.
   * Every shortcut that needs it only matches on the plate, where the slicing
   * shell has already loaded and created it, so the import resolves at once.
   */
  private withGcodePreview(use: (preview: GcodePreview) => void): void {
    void import('../gcode-preview').then((m) => use(this.injector.get(m.GcodePreview)));
  }
}

/**
 * Controls the tab order can land on. `tabindex="-1"` is excluded on purpose:
 * a roving radiogroup parks it on every option but the selected one, and
 * jumping into the unselected first segment would move the selection with the
 * next arrow key.
 */
const FOCUSABLE_SELECTOR =
  'button:not([disabled]):not([tabindex="-1"]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex="0"]';

/** Focusable widgets that already use the arrow keys for themselves. */
const ARROW_KEY_WIDGETS =
  '[role="radiogroup"], [role="radio"], [role="slider"], [role="listbox"], [role="menu"], [role="tablist"], [role="tab"], input[type="range"]';

const ARROW_GLYPHS: Readonly<Record<string, string>> = {
  Up: '↑',
  Down: '↓',
  Left: '←',
  Right: '→',
};

/** The 3D scene's own focusable host, which is where the keyboard belongs by default. */
function sceneHost(): HTMLElement | null {
  return document.querySelector<HTMLElement>('.viewer-host');
}

/**
 * Where `Tab` from the scene should land inside the open tool card.
 *
 * Each card marks its own entry with `data-tool-focus` — the first coordinate
 * field, the brush mode, the button that runs a placement — because "the first
 * thing that can be focused" is a header action on every one of them, and
 * landing on "reset rotation" is not what anyone meant by tabbing into the
 * panel. The fallbacks keep it working if a card ever forgets to say.
 */
function toolPanelTarget(): HTMLElement | null {
  const dock = document.querySelector('.tool-dock');
  if (!dock) {
    return null;
  }
  const marked = dock.querySelector<HTMLElement>('[data-tool-focus]');
  if (marked?.matches(FOCUSABLE_SELECTOR)) {
    return marked;
  }
  return (
    marked?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR) ??
    dock.querySelector<HTMLElement>(FOCUSABLE_SELECTOR)
  );
}

/**
 * `<input>` types that swallow no letter keys, so focusing one is not a reason
 * to hold the whole scene shortcut set.
 *
 * The gate exists so that correcting a typo in a settings field cannot reach
 * past the caret and undo the last scene operation. A slider is the opposite
 * case: it has no text to protect, so treating it as one only left every
 * letter shortcut dead — `p` could not get back out of G-code preview after a
 * touch of the layer slider, which is the control that view is worked from.
 */
const NON_TYPING_INPUT_TYPES = new Set([
  'range',
  'checkbox',
  'radio',
  'color',
  'button',
  'submit',
  'reset',
  'image',
  'file',
]);

/** What the tab shortcuts drive — implemented by the titlebar's `WorkplateTabs`. */
/** What the library page lets the keyboard do; see {@link KeyboardShortcuts.libraryPageRef}. */
export interface LibraryPageShortcuts {
  hasSelection(): boolean;
  deselect(): void;
  rename(): void;
  remove(): void;
  addModels(): void;
}

export interface WorkplateTabStrip {
  toggleSearch(): void;
  closeActive(): Promise<void>;
  closeAll(): Promise<void>;
  addTab(): Promise<void>;
  reopenClosed(): void;
  /** Move to the next (`1`) or previous (`-1`) tab, wrapping at the ends. */
  cycle(step: 1 | -1): void;
  /** Switch to the tab at `index`; negative counts from the end. */
  activateIndex(index: number): void;
}

/** An `<input>`'s effective type, lower-cased; missing or unknown reads as text. */
function inputTypeOf(element: HTMLElement): string {
  return ((element as HTMLInputElement).type || 'text').toLowerCase();
}

/**
 * Module-scoped helper used to initialise {@link KeyboardShortcuts.isMac}.
 * Kept as a plain function (not a class method) so the field initialiser
 * can call it before `this` is available in the constructor.
 */
function detectMac(): boolean {
  if (typeof navigator === 'undefined') {
    return false;
  }
  const uaData = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = uaData.userAgentData?.platform ?? navigator.platform ?? '';
  const userAgent = navigator.userAgent ?? '';
  // iPadOS reports "MacIntel" but is a touch device — the trackpad wheel
  // model does not apply there (touch pointer handlers run instead).
  if (platform === 'MacIntel' && navigator.maxTouchPoints > 1) {
    return false;
  }
  return /^Mac/i.test(platform) || /Mac OS X/i.test(userAgent);
}
