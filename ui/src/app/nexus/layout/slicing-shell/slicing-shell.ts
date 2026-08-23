import {
  Component,
  ElementRef,
  afterRenderEffect,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { ThreeDViewToolbar } from '../../../components/3d-view-toolbar/3d-view-toolbar';
import { Card } from '../../../components/card/card';
import { CodeEditor } from '../../../components/code-editor/code-editor';
import { SettingsPanel } from '../../../components/settings-panel/settings-panel';
import { SliceSegmentBar } from '../../../components/slice-segment-bar/slice-segment-bar';
import { TaskProgressBar } from '../../../components/task-progress-bar/task-progress-bar';
import { ViewportCube } from '../../../components/viewport-cube/viewport-cube';
import { SceneEngine } from '../../../services/scene-engine';
import { Slicer } from '../../../services/slicer';
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
    ViewportCube,
    RouterOutlet,
    SettingsPanel,
    CodeEditor,
    Card,
  ],
  templateUrl: './slicing-shell.html',
  styleUrl: './slicing-shell.scss',
})
export class NexusSlicingShell {
  private readonly sceneEngine = inject(SceneEngine);
  private readonly slicer = inject(Slicer);

  private readonly toolbarRef = viewChild(ThreeDViewToolbar, { read: ElementRef<HTMLElement> });

  readonly editorPanelVisible = signal(false);

  constructor() {
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

  /**
   * The current scene snapshot serialised as formatted JSON.
   *
   * This is exactly the payload that would be sent over WebSocket as scene
   * state when a slice job starts. `bigint` ids are serialised as strings so
   * JSON.stringify does not throw.
   */
  readonly snapshotJson = computed(() =>
    JSON.stringify(
      this.sceneEngine.snapshot(),
      (_key, value) => (typeof value === 'bigint' ? String(value) : value),
      2,
    ),
  );

  /**
   * The current slice settings serialised as formatted JSON.
   *
   * This is the `settings` payload that would be sent over WebSocket alongside
   * the scene snapshot when a slice job starts.
   */
  readonly sliceParamsJson = computed(() => JSON.stringify(this.slicer.settings(), null, 2));

  toggleEditorPanel(): void {
    this.editorPanelVisible.update((v) => !v);
  }
}
