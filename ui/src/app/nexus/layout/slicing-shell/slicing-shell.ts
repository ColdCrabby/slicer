import {
  Component,
  ElementRef,
  afterRenderEffect,
  effect,
  inject,
  untracked,
  viewChild,
} from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { ThreeDViewToolbar } from '../../../components/3d-view-toolbar/3d-view-toolbar';
import { Card } from '../../../components/card/card';
import { ObjectsPanel } from '../../../components/objects-panel/objects-panel';
import { SettingsPanel } from '../../../components/settings-panel/settings-panel';
import { SliceSegmentBar } from '../../../components/slice-segment-bar/slice-segment-bar';
import { TaskProgressBar } from '../../../components/task-progress-bar/task-progress-bar';
import { ViewportCube } from '../../../components/viewport-cube/viewport-cube';
import { PrintArea } from '../../../services/print-area';
import { ActiveSelection } from '../../../services/profiles/active-selection';
import { SceneEngine } from '../../../services/scene-engine';
import { Sidebar } from '../../sidebar/sidebar';
import { SliceControl } from '../../slice-control/slice-control';

@Component({
  selector: 'nexus-slicing-shell',
  imports: [
    Sidebar,
    SliceControl,
    SliceSegmentBar,
    TaskProgressBar,
    ThreeDViewToolbar,
    ObjectsPanel,
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
            /* update-banner already prompts a reload */
          }
        }
      });
    });

    // Keep --main-scene-inset on :root in sync with the toolbar's rendered
    // height so all floating panels (layer bar, segment bar, notification
    // center, etc.) stay inset below it regardless of its actual size.
    let obs: ResizeObserver | null = null;

    afterRenderEffect({
      read: (onCleanup) => {
        const el = this.toolbarRef()?.nativeElement;

        obs?.disconnect();
        obs = null;

        if (!el) return;

        obs = new ResizeObserver((entries) => {
          const h = entries[0]?.contentRect.height ?? 0;
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
