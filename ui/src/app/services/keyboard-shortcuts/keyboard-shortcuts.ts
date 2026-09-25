import { Injectable, inject } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { fromEvent } from 'rxjs';
import { filter, map } from 'rxjs/operators';
import { matchKeybindingPress, parseKeybinding } from 'tinykeys';
import { Arrange } from '../arrange';
import { GcodePreview } from '../gcode-preview';
import { SceneEngine } from '../scene-engine';
import { SceneHistory } from '../scene-history/scene-history';
import { Slicer } from '../slicer';
import { ViewerControl } from '../viewer-control';
import { WorkplateObjects } from '../workplate-objects/workplate-objects';

export interface ShortcutConfig {
  actionId: string;
  /** tinykeys-format shortcut string, e.g. `"$mod+z"`, `"$mod+Shift+z"`, `"a"`. */
  shortcut: string;
  /** Human-readable description of what the action does. */
  displayDescription: string;
  canMatch?: () => boolean;
  handleAction: () => void;
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
  private readonly gcodePreview = inject(GcodePreview);
  private readonly workplate = inject(WorkplateObjects);

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
      handleAction: () => {
        for (const id of this.viewerControl.selectedObjectIds()) {
          this.workplate.duplicate(id);
        }
      },
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
      displayDescription: 'Switch to translate mode',
      canMatch: () => this.onPlate(),
      handleAction: () => this.viewerControl.objectMode.set('translate'),
    },
    {
      actionId: 'object-mode-rotate',
      shortcut: 'r',
      displayDescription: 'Switch to rotate mode',
      canMatch: () => this.onPlate(),
      handleAction: () => this.viewerControl.objectMode.set('rotate'),
    },
    {
      actionId: 'object-mode-scale',
      shortcut: 's',
      displayDescription: 'Switch to scale mode',
      canMatch: () => this.onPlate(),
      handleAction: () => this.viewerControl.objectMode.set('scale'),
    },
    {
      actionId: 'object-mode-pull-to-floor',
      shortcut: 'f',
      displayDescription: 'Switch to pull-face-to-floor mode',
      canMatch: () => this.onPlate(),
      handleAction: () => this.viewerControl.objectMode.set('pullToFloor'),
    },
    {
      actionId: 'object-mode-paint',
      shortcut: 'b',
      displayDescription: 'Switch to paint-support mode',
      canMatch: () => this.onPlate(),
      handleAction: () => this.enterPaintMode(),
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
      actionId: 'focus-settings-search',
      shortcut: '$mod+f',
      displayDescription: 'Focus settings search',
      canMatch: () => this.settingsSearchRef !== null,
      handleAction: () => this.settingsSearchRef!.focusSearch(),
    },
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

  constructor() {
    fromEvent<KeyboardEvent>(document, 'keydown')
      .pipe(
        map((event) => ({ event, shortcut: this.findMatch(event) })),
        filter(({ shortcut }) => shortcut !== null),
        takeUntilDestroyed(),
      )
      .subscribe(({ event, shortcut }) => {
        event.preventDefault();
        shortcut!.handleAction();
      });
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
    const isApplePlatform = this.isApplePlatform();
    return config.shortcut.replace(/\$mod/g, isApplePlatform ? '⌘' : 'Ctrl');
  }

  /** Returns all registered shortcuts as plain data for display in a panel. */
  getAll(): { actionId: string; displayText: string; displayDescription: string }[] {
    return this.shortcuts.map(({ actionId, displayDescription }) => ({
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
    const status = this.slicer.status();
    if (!this.gcodePreview.gcodeHandle() && status !== 'slicing' && status !== 'uploading') {
      void this.slicer.slice();
    }
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

  /** Remove every selected object; undo brings them back. */
  private removeSelected(): void {
    const targets = this.viewerControl.selectedObjectIds();
    for (const id of targets) {
      this.workplate.remove(id);
    }
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

  private isApplePlatform(): boolean {
    const uaData = navigator as Navigator & {
      userAgentData?: {
        platform?: string;
      };
    };
    const platform = navigator.platform ?? '';
    const uaDataPlatform = uaData.userAgentData?.platform ?? '';
    const userAgent = navigator.userAgent ?? '';

    if (/mac|iphone|ipad|ipod/i.test(`${uaDataPlatform} ${platform}`)) {
      return true;
    }

    // iPadOS can report MacIntel while still being a touch device.
    if (platform === 'MacIntel' && navigator.maxTouchPoints > 1) {
      return true;
    }

    return /ipad/i.test(userAgent);
  }

  private gcodeNextExtrusion(): void {
    this.gcodePreview.stepSegment(1);
  }

  private gcodePrevExtrusion(): void {
    this.gcodePreview.stepSegment(-1);
  }

  private gcodeNextLayer(): void {
    this.gcodePreview.stepLayer(1);
  }

  private gcodePrevLayer(): void {
    this.gcodePreview.stepLayer(-1);
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
