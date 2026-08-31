import {
  BoxGeometry,
  Mesh,
  MeshBasicMaterial,
  Object3D,
  PerspectiveCamera,
  Raycaster,
  Scene,
  Vector3,
} from 'three';
import type { WebGLRenderer } from 'three';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { GizmoManager, isTransformControlsPlane, isVisibleWithin } from './gizmo';

const CANVAS_SIZE = 400;
const CENTRE = CANVAS_SIZE / 2;

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

  // A detached gizmo hides its root, but every picker underneath stays
  // `visible` and keeps reporting raycast hits.
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
    expect(isVisibleWithin(box(), new Object3D())).toBe(false);
  });
});

// Pins the three.js behaviour the guards exist to compensate for. If a future
// three release starts honouring `visible`, this says so rather than leaving a
// mysterious extra check behind.
describe('Raycaster visibility (three.js behaviour)', () => {
  it('still reports hits against hidden geometry', () => {
    const camera = new PerspectiveCamera(50, 1, 0.1, 100);
    camera.position.set(0, 0, 5);
    camera.updateMatrixWorld(true);

    const root = new Object3D();
    root.add(new Mesh(new BoxGeometry(2, 2, 2), new MeshBasicMaterial()));
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

/**
 * `hitTest` decides whether a touch or pen press landed on a transform handle,
 * and a false positive silently eats the press — no selection, no deselection,
 * no context menu. It runs for touch and pen only, so nothing here is reachable
 * with a mouse, which is why both bugs it guards against shipped unnoticed.
 */
describe('GizmoManager.hitTest', () => {
  let canvas: HTMLCanvasElement;
  let gizmo: GizmoManager;
  let camera: PerspectiveCamera;

  function hitTestAt(x: number, y: number): boolean {
    const event = new Event('pointerdown', { bubbles: true }) as PointerEvent;
    Object.assign(event, { pointerType: 'touch', pointerId: 1, clientX: x, clientY: y });
    return gizmo.hitTest(event, camera, { domElement: canvas } as unknown as WebGLRenderer);
  }

  beforeEach(() => {
    canvas = document.createElement('canvas');
    canvas.getBoundingClientRect = () =>
      ({
        x: 0,
        y: 0,
        top: 0,
        left: 0,
        right: CANVAS_SIZE,
        bottom: CANVAS_SIZE,
        width: CANVAS_SIZE,
        height: CANVAS_SIZE,
        toJSON: () => ({}),
      }) as DOMRect;
    document.body.appendChild(canvas);

    const scene = new Scene();
    camera = new PerspectiveCamera(50, 1, 0.1, 1000);
    // Looking down -Z at the origin, where an attached gizmo sits.
    camera.position.set(0, 0, 60);
    camera.updateMatrixWorld(true);

    gizmo = new GizmoManager(scene, camera, { domElement: canvas } as unknown as WebGLRenderer);
  });

  afterEach(() => {
    gizmo.dispose();
    canvas.remove();
  });

  // A detached gizmo is only *hidden*, and it parks its pickers at the origin —
  // the middle of the bed. Every tap there was eaten, so on a tablet no model
  // could be selected at all.
  it('reports no hit while nothing is selected, even dead centre', () => {
    gizmo.setMode('translate', null);
    expect(hitTestAt(CENTRE, CENTRE)).toBe(false);
  });

  it('reports no hit in a mode that shows no handles', () => {
    gizmo.setMode('none', new Vector3(0, 0, 0));
    expect(hitTestAt(CENTRE, CENTRE)).toBe(false);
  });

  describe('with a selection attached', () => {
    beforeEach(() => {
      gizmo.setMode('translate', new Vector3(0, 0, 0));
    });

    it('reports a hit on the handles themselves', () => {
      expect(hitTestAt(CENTRE, CENTRE)).toBe(true);
    });

    // The helper carries a 100000x100000 drag-projection plane whose *material*
    // is invisible but whose object is visible, so it is hit everywhere on
    // screen. That made it impossible to deselect, or to pick a different
    // model, once anything was selected.
    it('reports no hit in the far corner, where only the drag plane reaches', () => {
      expect(hitTestAt(4, 4)).toBe(false);
      expect(hitTestAt(CANVAS_SIZE - 4, CANVAS_SIZE - 4)).toBe(false);
    });
  });
});

describe('isTransformControlsPlane', () => {
  it('is false for ordinary objects', () => {
    expect(isTransformControlsPlane(new Object3D())).toBe(false);
  });

  it("is true for an object carrying three's marker", () => {
    const plane = new Object3D();
    (plane as Object3D & { isTransformControlsPlane?: boolean }).isTransformControlsPlane = true;
    expect(isTransformControlsPlane(plane)).toBe(true);
  });
});
