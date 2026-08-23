import { Component, ElementRef, afterRenderEffect, viewChild } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { ThreeDViewToolbar } from '../../../components/3d-view-toolbar/3d-view-toolbar';
import { Card } from '../../../components/card/card';
import { SettingsPanel } from '../../../components/settings-panel/settings-panel';
import { SliceSegmentBar } from '../../../components/slice-segment-bar/slice-segment-bar';
import { TaskProgressBar } from '../../../components/task-progress-bar/task-progress-bar';
import { TransformPanel } from '../../../components/transform-panel/transform-panel';
import { ViewportCube } from '../../../components/viewport-cube/viewport-cube';
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
    TransformPanel,
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
}
