import { type Camera, type InstancedMesh, Raycaster, Vector2 } from 'three';
import { GCODE_REF_KEY, type GcodeInstanceRef } from './gcode-layer-renderer';

/** A resolved hover hit against a G-code extrusion instance. */
export interface GcodeHoverHit {
  ref: GcodeInstanceRef;
  instanceId: number;
  clientX: number;
  clientY: number;
  /** Originating pointer type (`'mouse' | 'pen' | 'touch'`) — drives tooltip placement. */
  pointerType: string;
  /** Pen tilt toward +X in degrees (−90…90); 0 for mouse/touch. */
  tiltX: number;
  /** Pen tilt toward +Y in degrees (−90…90); 0 for mouse/touch. */
  tiltY: number;
}

/**
 * Raycasts the G-code layer meshes on pointer move and reports the extrusion
 * instance under the cursor. Throttled to one raycast per animation frame, and
 * only active while enabled (i.e. in the G-code scalar views) so it costs
 * nothing in model mode.
 */
export class GcodeHoverProbe {
  private readonly raycaster = new Raycaster();
  private readonly ndc = new Vector2();
  private pending: PointerEvent | null = null;
  private rafHandle = 0;
  private enabled = false;

  constructor(
    private readonly dom: HTMLElement,
    private readonly camera: Camera,
    private readonly meshes: () => InstancedMesh[],
    private readonly onHover: (hit: GcodeHoverHit | null) => void,
  ) {
    dom.addEventListener('pointermove', this.onMove);
    dom.addEventListener('pointerleave', this.onLeave);
  }

  setEnabled(on: boolean): void {
    if (this.enabled === on) {
      return;
    }
    this.enabled = on;
    if (!on) {
      this.pending = null;
      this.onHover(null);
    }
  }

  dispose(): void {
    this.dom.removeEventListener('pointermove', this.onMove);
    this.dom.removeEventListener('pointerleave', this.onLeave);
    if (this.rafHandle) {
      cancelAnimationFrame(this.rafHandle);
      this.rafHandle = 0;
    }
  }

  private readonly onMove = (event: PointerEvent): void => {
    if (!this.enabled) {
      return;
    }
    this.pending = event;
    this.rafHandle ||= requestAnimationFrame(this.flush);
  };

  private readonly onLeave = (): void => {
    this.pending = null;
    if (this.enabled) {
      this.onHover(null);
    }
  };

  private readonly flush = (): void => {
    this.rafHandle = 0;
    const event = this.pending;
    this.pending = null;
    if (!event || !this.enabled) {
      return;
    }

    const rect = this.dom.getBoundingClientRect();
    this.ndc.set(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1,
    );
    this.raycaster.setFromCamera(this.ndc, this.camera);

    const hits = this.raycaster.intersectObjects(this.meshes(), false);
    const hit = hits.find((h) => h.instanceId !== undefined);
    const ref = hit?.object.userData[GCODE_REF_KEY] as GcodeInstanceRef | undefined;
    if (!hit || hit.instanceId === undefined || !ref) {
      this.onHover(null);
      return;
    }

    this.onHover({
      ref,
      instanceId: hit.instanceId,
      clientX: event.clientX,
      clientY: event.clientY,
      pointerType: event.pointerType,
      tiltX: event.tiltX ?? 0,
      tiltY: event.tiltY ?? 0,
    });
  };
}
