import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { KeyboardShortcuts } from '../../services/keyboard-shortcuts/keyboard-shortcuts';
import { ViewerControl, type TwoFingerGesture } from '../../services/viewer-control';
import { Badge } from '../badge/badge';

/**
 * A row inside a {@link DisplaySection}. `displayParts` may contain multiple
 * entries when a single action has more than one way to trigger it — each
 * part is rendered as its own `<kbd>` pill, joined by "or".
 */
interface DisplayRow {
  actionId: string;
  displayDescription: string;
  displayParts: string[];
}

/**
 * A grouped block of rows in the panel. `scope` decides which platform
 * badge (if any) is drawn on the section header and whether the rows are
 * highlighted or dimmed relative to the user's current platform.
 */
interface DisplaySection {
  title: string;
  /** Undefined → universal (no badge, always highlighted). */
  scope?: 'mac' | 'non-mac';
  rows: DisplayRow[];
}

@Component({
  selector: 'nexus-keyboard-shortcuts',
  standalone: true,
  imports: [Badge],
  templateUrl: './keyboard-shortcuts.html',
  styleUrl: './keyboard-shortcuts.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class KeyboardShortcutsPanel {
  private readonly keyboardShortcuts = inject(KeyboardShortcuts);
  private readonly viewerControl = inject(ViewerControl);

  readonly isMac = this.keyboardShortcuts.isMac;

  /** Current bare-two-finger-swipe action, reflected in the gesture rows. */
  readonly twoFingerGesture = this.viewerControl.trackpadTwoFingerGesture;

  /** Rebuilt reactively so the orbit/pan rows track the gesture preference. */
  readonly sections = computed<DisplaySection[]>(() => this.buildSections());

  /**
   * True when the given section describes the user's current platform.
   * Undefined scope (universal sections like `Keyboard shortcuts`) is
   * always considered current so it is never dimmed.
   */
  isCurrentPlatform(scope: DisplaySection['scope']): boolean {
    if (scope === undefined) {
      return true;
    }
    return scope === 'mac' ? this.isMac : !this.isMac;
  }

  /** Change the two-finger swipe action (persists + updates the live viewer). */
  setTwoFingerGesture(gesture: TwoFingerGesture): void {
    this.viewerControl.setTrackpadTwoFingerGesture(gesture);
  }

  private buildSections(): DisplaySection[] {
    const twoFingerPans = this.viewerControl.trackpadTwoFingerGesture() === 'pan';
    const actionRows: DisplayRow[] = this.keyboardShortcuts.getAll().map((s) => ({
      actionId: s.actionId,
      displayDescription: s.displayDescription,
      displayParts: [s.displayText],
    }));

    return [
      { title: 'Keyboard shortcuts', rows: actionRows },
      {
        title: 'Trackpad gestures',
        scope: 'mac',
        rows: [
          {
            actionId: 'orbit',
            displayDescription: 'Orbit around target',
            displayParts: twoFingerPans
              ? ['Click + drag', '⌥ + Two-finger swipe']
              : ['Click + drag', 'Two-finger swipe'],
          },
          {
            actionId: 'pan',
            displayDescription: 'Pan camera',
            displayParts: twoFingerPans
              ? ['Two-finger swipe', 'Right-click + drag']
              : ['⌥ + Two-finger swipe', 'Right-click + drag'],
          },
          {
            actionId: 'zoom',
            displayDescription: 'Zoom to cursor',
            displayParts: ['Pinch', 'Ctrl + scroll'],
          },
        ],
      },
      {
        title: 'Mouse controls',
        scope: 'non-mac',
        rows: [
          {
            actionId: 'orbit',
            displayDescription: 'Orbit around target',
            displayParts: ['Left-click + drag'],
          },
          {
            actionId: 'pan',
            displayDescription: 'Pan camera',
            displayParts: ['Right-click + drag'],
          },
          {
            actionId: 'zoom',
            displayDescription: 'Zoom',
            displayParts: ['Scroll wheel'],
          },
          {
            actionId: 'autoscroll-zoom',
            displayDescription: 'Autoscroll zoom',
            displayParts: ['Middle-click + drag ↕'],
          },
        ],
      },
    ];
  }
}
