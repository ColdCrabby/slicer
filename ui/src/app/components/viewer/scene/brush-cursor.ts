import {
  BufferGeometry,
  Float32BufferAttribute,
  Line,
  LineBasicMaterial,
  type Material,
  type Object3D,
  type Scene,
  Vector3,
} from 'three';

/** Segments around the ring. 48 reads as a circle at any practical size. */
const SEGMENTS = 48;

/**
 * The brush's footprint, drawn on the surface under the pointer.
 *
 * Resizing by scroll is unusable without it: the number in the panel says
 * `2 mm`, but what the user needs to know is how much of *this* model that
 * covers. The ring is drawn with `depthTest` off so it never sinks into the
 * surface it is measuring, the same way every paint cursor behaves.
 */
export class BrushCursor {
  private readonly ring: Line;
  private readonly normalScratch = new Vector3();
  private readonly targetScratch = new Vector3();

  constructor(private readonly scene: Scene) {
    const positions = new Float32Array((SEGMENTS + 1) * 3);
    for (let i = 0; i <= SEGMENTS; i++) {
      const t = (i / SEGMENTS) * Math.PI * 2;
      positions[i * 3] = Math.cos(t);
      positions[i * 3 + 1] = Math.sin(t);
      positions[i * 3 + 2] = 0;
    }
    const geometry = new BufferGeometry();
    geometry.setAttribute('position', new Float32BufferAttribute(positions, 3));
    const material = new LineBasicMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: 0.9,
      depthTest: false,
      depthWrite: false,
    });
    this.ring = new Line(geometry, material);
    this.ring.renderOrder = 1000;
    this.ring.visible = false;
    scene.add(this.ring);
  }

  /**
   * Put the ring on the surface at `point`, lying in the plane whose normal is
   * `normal` (both world-space), sized to `radiusMm`.
   */
  show(point: Vector3, normal: Vector3, radiusMm: number): void {
    this.ring.position.copy(point);
    this.normalScratch.copy(normal);
    if (this.normalScratch.lengthSq() < 1e-12) {
      this.normalScratch.set(0, 0, 1);
    }
    this.normalScratch.normalize();
    // `lookAt` aims the ring's +Z (its own normal) down the view vector, so
    // pointing it along the surface normal lays the circle flat on the face.
    this.targetScratch.copy(point).add(this.normalScratch);
    this.ring.lookAt(this.targetScratch);
    this.ring.scale.setScalar(Math.max(radiusMm, 1e-3));
    this.ring.visible = true;
  }

  hide(): void {
    this.ring.visible = false;
  }

  get object(): Object3D {
    return this.ring;
  }

  dispose(): void {
    this.scene.remove(this.ring);
    this.ring.geometry.dispose();
    (this.ring.material as Material).dispose();
  }
}
