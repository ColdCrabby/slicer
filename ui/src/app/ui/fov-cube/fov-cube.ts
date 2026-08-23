import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  effect,
  inject,
  input,
} from '@angular/core';
import {
  BoxGeometry,
  Color,
  DirectionalLight,
  EdgesGeometry,
  Group,
  HemisphereLight,
  LineBasicMaterial,
  LineSegments,
  Mesh,
  MeshLambertMaterial,
  PerspectiveCamera,
  Scene,
  WebGLRenderer,
} from 'three';
import { AppTheme } from '../../services/app-theme';
import {
  pixelRatioCapFor,
  resolveAntialias,
  ViewerControl,
  type Antialiasing,
} from '../../services/viewer-control';

/** Distance the cube would sit at for a 45° FOV; other FOVs scale to match. */
const REFERENCE_FRAMING = 1.33;
const FALLBACK_ACCENT = 0xe0730f;
// Same model surface colours the main viewer paints meshes with, per theme.
const MODEL_COLOR_DARK = 0xb0bbc9;
const MODEL_COLOR_LIGHT = 0x9aa6b8;

/**
 * A tiny live 3D cube whose camera field-of-view mirrors the `fov` input, so
 * dragging a FOV slider next to it shows the perspective effect the number
 * produces. The cube's apparent size is held constant while the camera dollies
 * — exactly how the main viewer treats a FOV change — so only the perspective
 * distortion (near edges looming, far edges converging) varies.
 *
 * It mirrors the main viewer's render preferences: anti-aliasing, render
 * resolution (device-pixel-ratio cap), and the active colour scheme, reacting
 * live to each so the preview always matches what the real scene looks like.
 */
@Component({
  selector: 'nexus-fov-cube',
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './fov-cube.scss',
  template: '',
})
export class FovCube {
  readonly fov = input(45);

  private readonly hostRef = inject(ElementRef<HTMLElement>);
  private readonly viewerControl = inject(ViewerControl);
  private readonly appTheme = inject(AppTheme);
  private readonly destroyRef = inject(DestroyRef);

  private renderer: WebGLRenderer | null = null;
  private scene: Scene | null = null;
  private camera: PerspectiveCamera | null = null;
  private cube: Group | null = null;
  private body: Mesh | null = null;
  private hemiLight: HemisphereLight | null = null;
  private keyLight: DirectionalLight | null = null;
  private fillLight: DirectionalLight | null = null;
  private rafHandle = 0;
  private angle = 0;
  private lastAntialiasing: Antialiasing | null = null;

  constructor() {
    afterNextRender(() => this.init());

    // Keep the camera FOV in lock-step with the input.
    effect(() => {
      const fov = this.fov();
      if (this.camera) {
        this.applyFov(fov);
      }
    });

    // Mirror the render-resolution (pixel-ratio cap) preference.
    effect(() => {
      const cap = pixelRatioCapFor(this.viewerControl.renderQuality());
      this.applyPixelRatio(cap);
    });

    // Mirror the anti-aliasing preference. MSAA is a construction-only renderer
    // option, so a change rebuilds the renderer (and its canvas).
    effect(() => {
      const mode = this.viewerControl.antialiasing();
      if (this.lastAntialiasing === null) {
        this.lastAntialiasing = mode;
        return;
      }
      if (mode === this.lastAntialiasing) {
        return;
      }
      this.lastAntialiasing = mode;
      if (this.renderer) {
        this.buildRenderer();
      }
    });

    // Mirror the active colour scheme (lighting rig + model surface colour).
    effect(() => {
      const isDark = this.appTheme.isDarkMode();
      this.applyTheme(isDark);
    });

    this.destroyRef.onDestroy(() => this.dispose());
  }

  private init(): void {
    this.scene = new Scene();
    this.camera = new PerspectiveCamera(this.fov(), 1, 0.1, 100);

    const cube = new Group();
    const body = new Mesh(new BoxGeometry(1, 1, 1), new MeshLambertMaterial());
    const edges = new LineSegments(
      new EdgesGeometry(body.geometry),
      new LineBasicMaterial({ color: this.readAccent() }),
    );
    cube.add(body, edges);
    cube.rotation.x = -0.5;
    this.scene.add(cube);
    this.cube = cube;
    this.body = body;

    this.hemiLight = new HemisphereLight(0xffffff, 0x30343d, 1.1);
    this.scene.add(this.hemiLight);
    this.keyLight = new DirectionalLight(0xffffff, 1.4);
    this.keyLight.position.set(2, 3, 4);
    this.scene.add(this.keyLight);
    this.fillLight = new DirectionalLight(0xffffff, 0.2);
    this.fillLight.position.set(-1.8, 1.4, -2.2);
    this.scene.add(this.fillLight);

    this.lastAntialiasing = this.viewerControl.antialiasing();
    this.buildRenderer();
    this.applyTheme(this.appTheme.isDarkMode());
    this.applyFov(this.fov());
    this.tick();
  }

  /** Create (or recreate) the renderer with the current settings, replacing the canvas. */
  private buildRenderer(): void {
    const host = this.hostRef.nativeElement;
    const size = Math.max(host.clientWidth || 64, 48);
    if (this.renderer) {
      const old = this.renderer.domElement;
      this.renderer.dispose();
      old.remove();
    }
    this.renderer = new WebGLRenderer({
      antialias: resolveAntialias(this.viewerControl.antialiasing()),
      alpha: true,
    });
    this.renderer.setPixelRatio(
      Math.min(window.devicePixelRatio, pixelRatioCapFor(this.viewerControl.renderQuality())),
    );
    this.renderer.setSize(size, size, false);
    const canvas = this.renderer.domElement;
    canvas.style.width = '100%';
    canvas.style.height = '100%';
    host.appendChild(canvas);
  }

  private applyPixelRatio(cap: number): void {
    if (!this.renderer) {
      return;
    }
    const size = Math.max(this.hostRef.nativeElement.clientWidth || 64, 48);
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, cap));
    this.renderer.setSize(size, size, false);
  }

  /** Retune the lighting rig and cube surface colour for the active scheme. */
  private applyTheme(isDark: boolean): void {
    if (this.body) {
      (this.body.material as MeshLambertMaterial).color.setHex(
        isDark ? MODEL_COLOR_DARK : MODEL_COLOR_LIGHT,
      );
    }
    if (this.hemiLight) {
      this.hemiLight.groundColor.setHex(isDark ? 0x1c1e24 : 0xaab0bd);
      this.hemiLight.intensity = isDark ? 0.4 : 1.15;
    }
    if (this.keyLight) {
      this.keyLight.intensity = isDark ? 1.45 : 1.9;
    }
    if (this.fillLight) {
      this.fillLight.color.setHex(isDark ? 0xd6deec : 0xffffff);
      this.fillLight.intensity = isDark ? 0.16 : 0.3;
    }
  }

  /** Set the camera FOV and dolly it so the cube keeps a constant screen size. */
  private applyFov(fov: number): void {
    if (!this.camera) {
      return;
    }
    const tan = Math.tan(((fov / 2) * Math.PI) / 180);
    this.camera.fov = fov;
    this.camera.position.set(0, 0, REFERENCE_FRAMING / Math.max(tan, 1e-3));
    this.camera.lookAt(0, 0, 0);
    this.camera.updateProjectionMatrix();
  }

  private tick = (): void => {
    if (!this.renderer || !this.scene || !this.camera || !this.cube) {
      return;
    }
    this.angle += 0.01;
    this.cube.rotation.y = this.angle;
    this.renderer.render(this.scene, this.camera);
    this.rafHandle = requestAnimationFrame(this.tick);
  };

  /** Resolve the amber accent from the live CSS variable, with a static fallback. */
  private readAccent(): number {
    const raw = getComputedStyle(this.hostRef.nativeElement).getPropertyValue('--accent').trim();
    if (!raw) {
      return FALLBACK_ACCENT;
    }
    try {
      return new Color(raw).getHex();
    } catch {
      return FALLBACK_ACCENT;
    }
  }

  private dispose(): void {
    cancelAnimationFrame(this.rafHandle);
    this.cube?.traverse((obj) => {
      if (obj instanceof Mesh || obj instanceof LineSegments) {
        obj.geometry.dispose();
        (obj.material as MeshLambertMaterial | LineBasicMaterial).dispose();
      }
    });
    this.renderer?.dispose();
    this.renderer?.domElement.remove();
    this.renderer = null;
    this.scene = null;
    this.camera = null;
    this.cube = null;
    this.body = null;
  }
}
