import {
  BufferGeometry,
  DoubleSide,
  Float32BufferAttribute,
  Material,
  Mesh,
  MeshBasicMaterial,
  type Object3D,
} from 'three';

/** Support-enforcer overlay tint — matches the brush-mode convention used by mainstream slicers. */
const ENFORCER_COLOR = 0x2f80ed;
/** Support-blocker overlay tint. */
const BLOCKER_COLOR = 0xe74c3c;

function makeOverlayMesh(color: number): Mesh {
  const geometry = new BufferGeometry();
  const material = new MeshBasicMaterial({
    color,
    transparent: true,
    opacity: 0.6,
    side: DoubleSide,
    depthWrite: false,
    // A coplanar decal on the model's own surface z-fights against it at
    // `depthTest`-equal depth. `polygonOffset` nudges the overlay toward the
    // camera in the depth buffer only — unlike `faceHighlight`'s manual
    // normal-lift, this needs no per-vertex normal, and unlike disabling
    // `depthTest`, the overlay still stays correctly hidden behind the
    // model's *own* far side instead of showing through it.
    polygonOffset: true,
    polygonOffsetFactor: -4,
    polygonOffsetUnits: -4,
  });
  const mesh = new Mesh(geometry, material);
  mesh.renderOrder = 900;
  mesh.visible = false;
  return mesh;
}

function setMeshPositions(mesh: Mesh, positions: Float32Array): boolean {
  const existing = mesh.geometry.getAttribute('position');
  if (existing instanceof Float32BufferAttribute && existing.array.length === positions.length) {
    (existing.array as Float32Array).set(positions);
    existing.needsUpdate = true;
  } else {
    mesh.geometry.setAttribute('position', new Float32BufferAttribute(positions, 3));
  }
  mesh.geometry.computeBoundingSphere();
  return positions.length > 0;
}

function disposeMesh(mesh: Mesh): void {
  mesh.parent?.remove(mesh);
  mesh.geometry.dispose();
  (mesh.material as Material).dispose();
}

/**
 * Persistent per-object overlay of painted support enforcers/blockers,
 * visible only while the paint tool is active.
 *
 * Overlay meshes are added as **children** of each object's own `Object3D`
 * rather than tracked in world space, so they inherit its transform for
 * free — `SceneEngine.getPaintBuffer` already returns vertices in the
 * object's local frame for exactly this reason.
 */
export class PaintOverlay {
  private readonly byObject = new Map<string, { enforcers: Mesh; blockers: Mesh }>();
  private toolVisible = false;

  /** Show or hide every tracked overlay — call when entering/leaving paint mode. */
  setToolVisible(visible: boolean): void {
    this.toolVisible = visible;
    for (const pair of this.byObject.values()) {
      this.applyVisibility(pair.enforcers);
      this.applyVisibility(pair.blockers);
    }
  }

  private applyVisibility(mesh: Mesh): void {
    mesh.visible = this.toolVisible && mesh.userData['hasGeometry'] === true;
  }

  /** Rebuild one object's overlay geometry from a fresh paint buffer. */
  refresh(
    objectId: string,
    anchor: Object3D,
    buffer: { enforcers: Float32Array; blockers: Float32Array },
  ): void {
    let pair = this.byObject.get(objectId);
    if (!pair) {
      pair = {
        enforcers: makeOverlayMesh(ENFORCER_COLOR),
        blockers: makeOverlayMesh(BLOCKER_COLOR),
      };
      anchor.add(pair.enforcers);
      anchor.add(pair.blockers);
      this.byObject.set(objectId, pair);
    } else if (pair.enforcers.parent !== anchor) {
      // The object's Object3D was rebuilt (e.g. mesh-quality change) since the
      // overlay was created — follow it rather than orphaning the overlay on
      // the disposed old one.
      anchor.add(pair.enforcers);
      anchor.add(pair.blockers);
    }
    pair.enforcers.userData['hasGeometry'] = setMeshPositions(pair.enforcers, buffer.enforcers);
    pair.blockers.userData['hasGeometry'] = setMeshPositions(pair.blockers, buffer.blockers);
    this.applyVisibility(pair.enforcers);
    this.applyVisibility(pair.blockers);
  }

  /** Drop one object's overlay entirely — call when the object is removed. */
  remove(objectId: string): void {
    const pair = this.byObject.get(objectId);
    if (!pair) {
      return;
    }
    disposeMesh(pair.enforcers);
    disposeMesh(pair.blockers);
    this.byObject.delete(objectId);
  }

  /** Drop every overlay. Call when the whole scene is torn down. */
  disposeAll(): void {
    for (const id of [...this.byObject.keys()]) {
      this.remove(id);
    }
  }
}
