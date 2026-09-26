import {
  Component,
  ElementRef,
  afterRenderEffect,
  computed,
  effect,
  inject,
  untracked,
  viewChild,
} from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { ThreeDViewToolbar } from '../../../components/3d-view-toolbar/3d-view-toolbar';
import { Card } from '../../../components/card/card';
import { ObjectsPanel } from '../../../components/objects-panel/objects-panel';
import { PaintPanel } from '../../../components/paint-panel/paint-panel';
import { PlacementPanel } from '../../../components/placement-panel/placement-panel';
import { SettingsPanel } from '../../../components/settings-panel/settings-panel';
import { LibraryFlyout } from '../../../components/library/library-flyout';
import { GcodeTextPanel } from '../../../components/gcode-text-panel/gcode-text-panel';
import { SliceSegmentBar } from '../../../components/slice-segment-bar/slice-segment-bar';
import { SceneNotices } from '../../../components/notices/scene-notices/scene-notices';
import { TransformPanel } from '../../../components/transform-panel/transform-panel';
import { ViewportCube } from '../../../components/viewport-cube/viewport-cube';
import { GcodePreview } from '../../../services/gcode-preview';
import { LibraryFlyout as LibraryFlyoutState } from '../../../services/library/library-flyout';
import { PrintArea } from '../../../services/print-area';
import { ActiveSelection } from '../../../services/profiles/active-selection';
import { SceneEngine } from '../../../services/scene-engine';
import { ViewerControl } from '../../../services/viewer-control';
import { Sidebar } from '../../sidebar/sidebar';
import { Panel } from '../../../ui/panel/panel';
import { SliceControl } from '../../slice-control/slice-control';

@Component({
  selector: 'nexus-slicing-shell',
  imports: [
    Panel,
    Sidebar,
    SliceControl,
    SliceSegmentBar,
    GcodeTextPanel,
    LibraryFlyout,
    SceneNotices,
    ThreeDViewToolbar,
    ObjectsPanel,
    TransformPanel,
    PlacementPanel,
    PaintPanel,
    ViewportCube,
    RouterOutlet,
    SettingsPanel,
    Card,
  ],
  templateUrl: './slicing-shell.html',
  styleUrl: './slicing-shell.scss',
})
export class NexusSlicingShell {
  private readonly toolbarRef = viewChild(ThreeDViewToolbar, { read: ElementRef<HTMLElement> });
  private readonly activeSelection = inject(ActiveSelection);
  private readonly printArea = inject(PrintArea);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly viewerControl = inject(ViewerControl);
  private readonly preview = inject(GcodePreview);
  protected readonly libraryFlyout = inject(LibraryFlyoutState);

  /**
   * The G-code text column is only docked once there is a file to read. Without
   * this it would open on an empty panel after a reload — the preference is
   * remembered but the slice is not.
   */
  protected readonly textPanelOpen = computed(
    () => this.viewerControl.gcodeTextPanel() && this.preview.gcodeText() !== null,
  );

  constructor() {
    // Apply the active printer's bed to the print area and the scene engine.
    // Lives here (not in a root service) so opening Settings never boots the
    // slicer runtime — this shell is only ever constructed inside the slice
    // workspace.
    //
    // The slice *parameters* are deliberately not pushed anywhere: `Slicer`
    // derives them from the same selection plus the plate's override diff.
    // Copying the resolved stack into a writable settings signal, as this
    // effect used to, left values from the previously-selected preset behind
    // whenever the new one was silent about a key — and those strays then read
    // as deliberate user overrides.
    //
    // Only `printAreaConfig()` / `sceneBedConfig()` are tracked. The writes run
    // inside `untracked()` because `updateConfig` reads its own target signal;
    // tracking that read would make the effect depend on the signal it writes
    // and loop forever.
    effect(() => {
      const printAreaBed = this.activeSelection.printAreaConfig();
      const sceneBed = this.activeSelection.sceneBedConfig();
      untracked(() => {
        if (printAreaBed) {
          this.printArea.updateConfig(printAreaBed);
        }
        if (sceneBed) {
          // A stale bundle without setBed throws; don't let that block the
          // print-area write above.
          try {
            this.sceneEngine.setBed(sceneBed);
          } catch {
            /* the reload prompt in the window dock already covers this */
          }
        }
      });
    });

    // Keep --main-scene-inset on :root in sync with the toolbar's rendered
    // height so all floating panels (layer bar, segment bar, scene notices,
    // etc.) stay inset below it regardless of its actual size.
    let obs: ResizeObserver | null = null;

    afterRenderEffect({
      read: (onCleanup) => {
        const el = this.toolbarRef()?.nativeElement;

        obs?.disconnect();
        obs = null;

        if (!el) return;

        obs = new ResizeObserver((entries) => {
          // Border box, not `contentRect`. The toolbar carries its own vertical
          // padding, so the content box is ~24px shorter than the space it
          // actually occupies — and everything keyed to this variable sat that
          // much too high, underneath the toolbar's own buttons.
          const h = entries[0]?.borderBoxSize?.[0]?.blockSize ?? el.offsetHeight;
          if (h > 0) document.documentElement.style.setProperty('--main-scene-inset', `${h}px`);
        });
        obs.observe(el);

        onCleanup(() => {
          obs?.disconnect();
          obs = null;
          document.documentElement.style.removeProperty('--main-scene-inset');
        });
      },
    });
  }
}
