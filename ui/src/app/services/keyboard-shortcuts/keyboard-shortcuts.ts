import { Injectable, inject } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { fromEvent } from 'rxjs';
import { filter, map } from 'rxjs/operators';
import { matchKeyBindingPress, parseKeybinding } from 'tinykeys';
import { GcodePreview } from '../gcode-preview';
import { SceneCommand } from '../scene-command/scene-command';
import { SceneHistory } from '../scene-history/scene-history';
import { Slicer } from '../slicer';
import { ViewerControl } from '../viewer-control';

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
  private readonly sceneCommand = inject(SceneCommand);
  private readonly viewerControl = inject(ViewerControl);
  private readonly slicer = inject(Slicer);
  private readonly gcodePreview = inject(GcodePreview);

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
      canMatch: () => this.history.canUndo(),
      handleAction: () => this.history.undo(),
    },
    {
      actionId: 'redo',
      shortcut: '$mod+y',
      displayDescription: 'Redo',
      canMatch: () => this.history.canRedo(),
      handleAction: () => this.history.redo(),
    },
    {
      actionId: 'redo-alt',
      shortcut: '$mod+Shift+z',
      displayDescription: 'Redo (alternate)',
      canMatch: () => this.history.canRedo(),
      handleAction: () => this.history.redo(),
    },
    {
      actionId: 'auto-orient',
      shortcut: 'a',
      displayDescription: 'Auto-orient all objects',
      canMatch: () => !this.isTextInputFocused(),
      handleAction: () => this.sceneCommand.autoOrient(),
    },
    {
      actionId: 'object-mode-translate',
      shortcut: 'm',
      displayDescription: 'Switch to translate mode',
      canMatch: () => !this.isTextInputFocused(),
      handleAction: () => this.viewerControl.objectMode.set('translate'),
    },
    {
      actionId: 'object-mode-rotate',
      shortcut: 'r',
      displayDescription: 'Switch to rotate mode',
      canMatch: () => !this.isTextInputFocused(),
      handleAction: () => this.viewerControl.objectMode.set('rotate'),
    },
    {
      actionId: 'object-mode-scale',
      shortcut: 's',
      displayDescription: 'Switch to scale mode',
      canMatch: () => !this.isTextInputFocused(),
      handleAction: () => this.viewerControl.objectMode.set('scale'),
    },
    {
      actionId: 'object-mode-pull-to-floor',
      shortcut: 'f',
      displayDescription: 'Switch to pull-face-to-floor mode',
      canMatch: () => !this.isTextInputFocused(),
      handleAction: () => this.viewerControl.objectMode.set('pullToFloor'),
    },
    {
      actionId: 'toggle-gravity',
      shortcut: 'g',
      displayDescription: 'Toggle gravity',
      canMatch: () => !this.isTextInputFocused(),
      handleAction: () => this.viewerControl.gravityEnabled.update((v) => !v),
    },
    {
      actionId: 'toggle-view-mode',
      shortcut: 'p',
      displayDescription: 'Toggle G-code preview / model view',
      canMatch: () => !this.isTextInputFocused(),
      handleAction: () => this.toggleViewMode(),
    },
    {
      actionId: 'toggle-projection',
      shortcut: 'Shift+Space',
      displayDescription: 'Switch between orthographic and perspective views',
      canMatch: () => !this.isTextInputFocused(),
      handleAction: () => this.toggleProjection(),
    },
    {
      actionId: 'gcode-next-extrusion',
      shortcut: 'ArrowRight',
      displayDescription: 'Next extrusion (G-code viewer)',
      canMatch: () => this.viewerControl.viewMode() === 'gcode',
      handleAction: () => this.gcodeNextExtrusion(),
    },
    {
      actionId: 'gcode-prev-extrusion',
      shortcut: 'ArrowLeft',
      displayDescription: 'Previous extrusion (G-code viewer)',
      canMatch: () => this.viewerControl.viewMode() === 'gcode',
      handleAction: () => this.gcodePrevExtrusion(),
    },
    {
      actionId: 'gcode-next-layer',
      shortcut: 'ArrowUp',
      displayDescription: 'Next layer (G-code viewer)',
      canMatch: () => this.viewerControl.viewMode() === 'gcode',
      handleAction: () => this.gcodeNextLayer(),
    },
    {
      actionId: 'gcode-prev-layer',
      shortcut: 'ArrowDown',
      displayDescription: 'Previous layer (G-code viewer)',
      canMatch: () => this.viewerControl.viewMode() === 'gcode',
      handleAction: () => this.gcodePrevLayer(),
    },
    {
      actionId: 'focus-settings-search',
      shortcut: '$mod+f',
      displayDescription: 'Focus settings search',
      canMatch: () => this.schemaFormRef !== null,
      handleAction: () => this.schemaFormRef!.focusSearch(),
    },
  ].map((s) => ({ ...s, _parsed: parseKeybinding(s.shortcut) }));

  /** Set by SchemaForm on mount/destroy so the shortcut knows when search is available. */
  schemaFormRef: { focusSearch(): void } | null = null;

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
    return config.shortcut.replace(/\$mod/g, isApplePlatform ? '⌘' : 'Ctrl').replace(/\+/g, '+');
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
          s._parsed.every((press) => matchKeyBindingPress(event, press)) &&
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

  private isTextInputFocused(): boolean {
    const target = document.activeElement as HTMLElement | null;
    if (!target) {
      return false;
    }
    const tag = target.tagName.toUpperCase();
    return tag === 'INPUT' || tag === 'TEXTAREA' || target.isContentEditable;
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
    const handle = this.gcodePreview.gcodeHandle();
    const layerIndex = this.gcodePreview.layerMax();
    if (!handle) {
      return;
    }
    const layer = handle.getLayer(layerIndex);
    const blockCount = layer.blocksCount();
    if (blockCount === 0) {
      return;
    }
    const current = this.gcodePreview.segmentProgress();
    const step = 1 / blockCount;
    const next = Math.min(1, current + step);
    this.gcodePreview.setSegmentProgress(next);
  }

  private gcodePrevExtrusion(): void {
    const handle = this.gcodePreview.gcodeHandle();
    const layerIndex = this.gcodePreview.layerMax();
    if (!handle) {
      return;
    }
    const layer = handle.getLayer(layerIndex);
    const blockCount = layer.blocksCount();
    if (blockCount === 0) {
      return;
    }
    const current = this.gcodePreview.segmentProgress();
    const step = 1 / blockCount;
    const prev = Math.max(0, current - step);
    this.gcodePreview.setSegmentProgress(prev);
  }

  private gcodeNextLayer(): void {
    const current = this.gcodePreview.layerMax();
    const count = this.gcodePreview.layerCount();
    if (current < count - 1) {
      this.gcodePreview.setLayerMax(current + 1);
    }
  }

  private gcodePrevLayer(): void {
    const current = this.gcodePreview.layerMax();
    if (current > 0) {
      this.gcodePreview.setLayerMax(current - 1);
    }
  }
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
