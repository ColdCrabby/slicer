import { Color, ConeGeometry, Group, Mesh, MeshBasicMaterial } from 'three';

/**
 * A simple imaginary nozzle marking exactly where the toolhead sits while
 * scrubbing the G-code preview.
 *
 * Some moves — a wipe, a tiny travel, a seam retract — change nothing a
 * viewer can see, especially with travel hidden. Without a marker, stepping
 * through them looks like nothing happened at all. The cone is not a model of
 * any real nozzle; it only needs to read, at a glance, as "the tip is here."
 */
export class NozzleMarker {
  readonly group = new Group();

  constructor() {
    const color = readAccentColor();
    const material = new MeshBasicMaterial({
      color,
      transparent: true,
      opacity: 0.85,
      depthTest: false,
    });
    // Apex at the local origin (the print point), body opening upward — a
    // schematic nozzle silhouette, not a rendering of a real one.
    const geometry = new ConeGeometry(1.4, 4, 20);
    geometry.rotateX(Math.PI);
    geometry.translate(0, 2, 0);
    const cone = new Mesh(geometry, material);
    cone.renderOrder = 999;
    this.group.add(cone);
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
    for (const child of this.group.children) {
      if (child instanceof Mesh) {
        child.geometry.dispose();
        (child.material as MeshBasicMaterial).dispose();
      }
    }
  }
}

/** `--accent` resolves at construction time; the marker is short-lived per preview session. */
function readAccentColor(): Color {
  if (typeof document === 'undefined') {
    return new Color(0xffffff);
  }
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim();
  if (raw.startsWith('#')) {
    return new Color(parseInt(raw.substring(1), 16));
  }
  return new Color(0xffffff);
}
