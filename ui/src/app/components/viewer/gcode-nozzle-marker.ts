import { Color, CylinderGeometry, Group, Mesh, MeshBasicMaterial } from 'three';

/** Fixed off-white — reads clearly against every role color and both themes. */
const MARKER_COLOR = 0xefefef;

/** Cone (the nozzle body) height and radius, in mm. */
const NOZZLE_HEIGHT = 6;
const NOZZLE_RADIUS = 1.4;

/** Filament strand feeding into the nozzle from above, in mm. */
const FILAMENT_HEIGHT = 10;
const FILAMENT_RADIUS = 0.35;

/**
 * A simple imaginary nozzle marking exactly where the toolhead sits while
 * scrubbing the G-code preview: a downward-pointing cone with a thin filament
 * strand feeding into it from above.
 *
 * Some moves — a wipe, a tiny travel, a seam retract — change nothing a
 * viewer can see, especially with travel hidden. Without a marker, stepping
 * through them looks like nothing happened at all. This is not a model of any
 * real nozzle; it only needs to read, at a glance, as "the tip is here" rather
 * than as an unexplained shape floating over the plate.
 */
export class NozzleMarker {
  readonly group = new Group();
  #meshes: Mesh[] = [];

  constructor() {
    const material = new MeshBasicMaterial({
      color: new Color(MARKER_COLOR),
      transparent: true,
      opacity: 0.85,
      depthTest: false,
    });

    // The scene's vertical axis is Z (G-code XYZ maps straight onto Three.js
    // xyz — see gcode-layer-renderer.ts), not Three's default "up" of Y that
    // every primitive geometry is authored against. A geometry built without
    // correcting for that renders on its side here — rotateX(-90°) swaps the
    // cone/cylinder's Y-axis for Z, so "pointing down" actually means down.
    const nozzle = new CylinderGeometry(0, NOZZLE_RADIUS, NOZZLE_HEIGHT, 20);
    nozzle.rotateX(-Math.PI / 2);
    nozzle.translate(0, 0, NOZZLE_HEIGHT / 2);
    const nozzleMesh = new Mesh(nozzle, material);
    nozzleMesh.renderOrder = 999;
    this.group.add(nozzleMesh);

    // A thin strand feeding into the nozzle's wide end, so the cone alone
    // doesn't read as a floating marker gem — it's a nozzle because something
    // visibly feeds into it.
    const filament = new CylinderGeometry(FILAMENT_RADIUS, FILAMENT_RADIUS, FILAMENT_HEIGHT, 8);
    filament.rotateX(-Math.PI / 2);
    filament.translate(0, 0, NOZZLE_HEIGHT + FILAMENT_HEIGHT / 2);
    const filamentMesh = new Mesh(filament, material);
    filamentMesh.renderOrder = 999;
    this.group.add(filamentMesh);

    this.#meshes = [nozzleMesh, filamentMesh];
    this.group.visible = false;
  }

  /** Move the marker to `pos`, or hide it when nothing is revealed yet. */
  setPosition(pos: readonly [number, number, number] | null): void {
    if (!pos) {
      this.group.visible = false;
      return;
    }
    this.group.visible = true;
    this.group.position.set(pos[0], pos[1], pos[2]);
  }

  dispose(): void {
    for (const mesh of this.#meshes) {
      mesh.geometry.dispose();
    }
    (this.#meshes[0]?.material as MeshBasicMaterial | undefined)?.dispose();
  }
}
