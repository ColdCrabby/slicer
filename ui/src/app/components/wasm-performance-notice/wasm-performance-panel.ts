import { ChangeDetectionStrategy, Component } from '@angular/core';

/**
 * Dialog body shown once per browser session on the WebAssembly web build.
 *
 * Explains — with a concrete before/after metric — that running the slicer
 * entirely in the browser trades a large amount of raw performance for zero
 * install, and points users at the native app when they want full speed.
 */
@Component({
  selector: 'nexus-wasm-performance-panel',
  standalone: true,
  templateUrl: './wasm-performance-panel.html',
  styleUrl: './wasm-performance-panel.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WasmPerformancePanel {}
