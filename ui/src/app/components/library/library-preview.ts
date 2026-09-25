import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  effect,
  inject,
  input,
  signal,
  viewChild,
} from '@angular/core';
import type { PerspectiveCamera, Scene, WebGLRenderer } from 'three';

/** Elevation limits, radians from the horizon — never quite straight down or up. */
const MIN_ELEVATION = -1.2;
const MAX_ELEVATION = 1.45;

/**
 * A live, turnable view of one model — the selected library entry.
 *
 * There is only ever one on screen, which is what makes a live view
 * affordable: it holds one WebGL context and one parsed model, where a grid of
 * them would hold hundreds. Drag to turn it; that is all it does. It is a
 * look, not an editor.
 *
 * Presentational: it draws the `file` it is given. The scene engine must be
 * ready before a file arrives, since the file is parsed by the engine's own
 * loaders.
 */
@Component({
  selector: 'nexus-library-preview',
  template: `<canvas #canvas (pointerdown)="grab($event)"></canvas>
    @if (failed()) {
      <p class="preview-note">This model could not be shown.</p>
    } @else if (!drawn()) {
      @if (placeholder(); as src) {
        <img class="preview-placeholder" [src]="src" alt="" draggable="false" />
      }
      <span class="preview-spinner" aria-hidden="true"></span>
    }`,
  styles: `
    :host {
      position: relative;
      display: block;
      aspect-ratio: 1;
      border-radius: var(--radius-lg);
      background: var(--color-bg-secondary);
      overflow: hidden;
    }
    canvas {
      display: block;
      width: 100%;
      height: 100%;
      touch-action: none;
      cursor: grab;
    }
    canvas:active {
      cursor: grabbing;
    }
    .preview-placeholder {
      position: absolute;
      inset: 0;
      width: 100%;
      height: 100%;
      object-fit: contain;
      pointer-events: none;
    }
    .preview-spinner {
      position: absolute;
      right: var(--spacing-sm);
      bottom: var(--spacing-sm);
      width: 16px;
      height: 16px;
      border-radius: 50%;
      border: 2px solid var(--color-border);
      border-top-color: var(--accent);
      animation: preview-spin 0.9s linear infinite;
    }
    @keyframes preview-spin {
      to {
        transform: rotate(360deg);
      }
    }
    @media (prefers-reduced-motion: reduce) {
      .preview-spinner {
        animation-duration: 3s;
      }
    }
    .preview-note {
      position: absolute;
      inset: 0;
      display: grid;
      place-items: center;
      margin: 0;
      font-size: var(--font-size-sm);
      color: var(--color-text-tertiary);
    }
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LibraryPreview {
  readonly file = input<File | null>(null);
  /**
   * The entry's thumbnail, shown until the model itself is drawn. The picture
   * is already loaded — it is the one on the card — so the pane is never empty
   * while the file is read and parsed.
   */
  readonly placeholder = input<string | null>(null);

  protected readonly failed = signal(false);
  protected readonly drawn = signal(false);
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('canvas');

  #renderer: WebGLRenderer | null = null;
  #scene: Scene | null = null;
  #camera: PerspectiveCamera | null = null;
  #radius = 1;
  #disposeModel: (() => void) | null = null;
  #azimuth = -Math.PI / 4;
  #elevation = 0.55;
  #load = 0;

  constructor() {
    effect(() => {
      const file = this.file();
      void this.#show(file);
    });

    const resize = new ResizeObserver(() => this.#draw());
    effect(() => resize.observe(this.canvas().nativeElement));

    inject(DestroyRef).onDestroy(() => {
      resize.disconnect();
      this.#disposeModel?.();
      this.#renderer?.dispose();
    });
  }

  async #show(file: File | null): Promise<void> {
    const load = ++this.#load;
    this.failed.set(false);
    this.drawn.set(false);
    this.#disposeModel?.();
    this.#disposeModel = null;
    this.#scene = null;
    if (!file) {
      this.#draw();
      return;
    }
    try {
      const [three, geometry, bytes] = await Promise.all([
        import('three'),
        import('../../services/library/library-geometry'),
        file.arrayBuffer(),
      ]);
      if (load !== this.#load) {
        return;
      }
      const { scene, radius, dispose } = geometry.studio(
        geometry.parseModel(file.name, new Uint8Array(bytes)),
      );
      this.#scene = scene;
      this.#radius = radius;
      this.#disposeModel = dispose;
      this.#renderer ??= new three.WebGLRenderer({
        canvas: this.canvas().nativeElement,
        antialias: true,
        alpha: true,
      });
      this.#camera ??= new three.PerspectiveCamera(30, 1);
      this.#draw();
      this.drawn.set(true);
    } catch {
      if (load === this.#load) {
        this.failed.set(true);
      }
    }
  }

  /** Turn the model while a pointer is held down. */
  protected grab(event: PointerEvent): void {
    const canvas = event.currentTarget as HTMLCanvasElement;
    canvas.setPointerCapture(event.pointerId);
    let x = event.clientX;
    let y = event.clientY;
    const move = (e: PointerEvent) => {
      this.#azimuth -= (e.clientX - x) * 0.01;
      this.#elevation = Math.min(
        MAX_ELEVATION,
        Math.max(MIN_ELEVATION, this.#elevation + (e.clientY - y) * 0.01),
      );
      x = e.clientX;
      y = e.clientY;
      this.#draw();
    };
    const release = () => {
      canvas.removeEventListener('pointermove', move);
      canvas.removeEventListener('pointerup', release);
      canvas.removeEventListener('pointercancel', release);
    };
    canvas.addEventListener('pointermove', move);
    canvas.addEventListener('pointerup', release);
    canvas.addEventListener('pointercancel', release);
  }

  #draw(): void {
    const renderer = this.#renderer;
    const camera = this.#camera;
    if (!renderer || !camera) {
      return;
    }
    const canvas = renderer.domElement;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    if (width === 0 || height === 0) {
      return;
    }
    renderer.setPixelRatio(Math.min(globalThis.devicePixelRatio ?? 1, 2));
    renderer.setSize(width, height, false);
    renderer.setClearColor(0x000000, 0);
    camera.aspect = width / height;

    const distance = (this.#radius * 1.1) / Math.sin((camera.fov * Math.PI) / 360);
    const flat = Math.cos(this.#elevation);
    camera.up.set(0, 0, 1);
    camera.position.set(
      Math.cos(this.#azimuth) * flat * distance,
      Math.sin(this.#azimuth) * flat * distance,
      Math.sin(this.#elevation) * distance,
    );
    camera.near = distance / 100;
    camera.far = distance * 4;
    camera.lookAt(0, 0, 0);
    camera.updateProjectionMatrix();

    if (this.#scene) {
      renderer.render(this.#scene, camera);
    } else {
      renderer.clear();
    }
  }
}
