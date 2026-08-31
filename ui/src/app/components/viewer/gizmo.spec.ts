import {
  BoxGeometry,
  Mesh,
  MeshBasicMaterial,
  Object3D,
  PerspectiveCamera,
  Raycaster,
} from 'three';
import { describe, expect, it } from 'vitest';
import { isVisibleWithin } from './gizmo';

describe('isVisibleWithin', () => {
  function box(): Mesh {
    return new Mesh(new BoxGeometry(1, 1, 1), new MeshBasicMaterial());
  }

  it('accepts a visible object inside a visible root', () => {
    const root = new Object3D();
    const child = box();
    root.add(child);
    expect(isVisibleWithin(child, root)).toBe(true);
  });

  it('rejects an object hidden by itself', () => {
    const root = new Object3D();
    const child = box();
    child.visible = false;
    root.add(child);
    expect(isVisibleWithin(child, root)).toBe(false);
  });

  // The case that broke tap-to-select: a detached gizmo hides its root, but
  // every picker underneath stays `visible` and keeps reporting raycast hits.
  it('rejects a visible object inside a hidden root', () => {
    const root = new Object3D();
    const child = box();
    root.add(child);
    root.visible = false;
    expect(isVisibleWithin(child, root)).toBe(false);
  });

  it('rejects an object hidden by an intermediate ancestor', () => {
    const root = new Object3D();
    const middle = new Object3D();
    const child = box();
    middle.visible = false;
    root.add(middle);
    middle.add(child);
    expect(isVisibleWithin(child, root)).toBe(false);
  });

  it('rejects an object that is not in the subtree at all', () => {
    const root = new Object3D();
    const stranger = box();
    expect(isVisibleWithin(stranger, root)).toBe(false);
  });
});

// Pins the three.js behaviour the guard exists to compensate for. If a future
// three release starts honouring `visible`, this test says so rather than
// leaving a mysterious extra check behind.
describe('Raycaster visibility (three.js behaviour)', () => {
  it('still reports hits against hidden geometry', () => {
    const camera = new PerspectiveCamera(50, 1, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.updateMatrixWorld(true);

    const root = new Object3D();
    const target = new Mesh(new BoxGeometry(2, 2, 2), new MeshBasicMaterial());
    root.add(target);
    root.visible = false;
    root.updateMatrixWorld(true);

    const raycaster = new Raycaster();
    raycaster.setFromCamera({ x: 0, y: 0 } as never, camera);

    expect(raycaster.intersectObject(root, true).length).toBeGreaterThan(0);
    expect(
      raycaster.intersectObject(root, true).some((hit) => isVisibleWithin(hit.object, root)),
    ).toBe(false);
  });
});
