import { BoxGeometry, Mesh, MeshBasicMaterial, PerspectiveCamera, Scene } from 'three';
import type { WebGLRenderer } from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';
import type { GizmoManager } from '../gizmo';
import { SceneControls } from './controls';
import { SceneSelection } from './selection';
import type { SceneSelectionHandlers } from './types';

const CANVAS_SIZE = 200;
const CENTRE = CANVAS_SIZE / 2;

function pointerEvent(type: string, props: Record<string, unknown>): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.assign(event, {
    pointerId: 1,
    pointerType: 'mouse',
    button: 0,
    clientX: CENTRE,
    clientY: CENTRE,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...props,
  });
  return event;
}

/**
 * End-to-end demonstration of T-27 at the boundary that actually matters: the
 * `PaintSupport` ops the viewer applies to the scene engine. Every wobble of
 * the camera — orbit, pan, two-finger takeover, wheel — must cross the model
 * without emitting a single `PaintSupport` op.
 *
 * The setup mirrors production wiring: `SceneControls` is constructed before
 * `SceneSelection`, and a bubble-phase `pointerdown` listener on the very
 * canvas the selection listens to stands in for OrbitControls (the shape it
 * registers in `OrbitControls.connect`). Whether that listener runs *is*
 * whether the camera acts on the press; the `PaintSupport` count is whether
 * paint comes with it.
 */
describe('camera gestures never paint (T-27, end-to-end)', () => {
  let canvas: HTMLCanvasElement;
  let camera: PerspectiveCamera;
  let orbit: OrbitControls;
  let selection: SceneSelection;
  let controls: SceneControls;
  let cameraListener: Mock<(event: Event) => void>;
  let paintOps: number;

  function onModel(offsetPx = 0): Record<string, unknown> {
    return { clientX: CENTRE + offsetPx, clientY: CENTRE };
  }

  beforeEach(() => {
    paintOps = 0;
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

    const captured = new Set<number>();
    canvas.setPointerCapture = (pointerId: number) => {
      captured.add(pointerId);
    };
    canvas.releasePointerCapture = (pointerId: number) => {
      captured.delete(pointerId);
    };
    canvas.hasPointerCapture = (pointerId: number) => captured.has(pointerId);

    camera = new PerspectiveCamera(50, 1, 0.1, 1000);
    camera.position.set(0, 0, 10);
    camera.updateMatrixWorld(true);

    // OrbitControls' `pointerdown` on the canvas, bubble phase, registered
    // before the selection — the exact ordering production uses.
    cameraListener = vi.fn();
    canvas.addEventListener('pointerdown', cameraListener);

    const renderer = { domElement: canvas } as unknown as WebGLRenderer;

    orbit = new OrbitControls(camera, canvas);
    controls = new SceneControls(camera, orbit, renderer);
    controls.installAlwaysOnWheelZoom();

    const scene = new Scene();
    const gizmo = {
      isHovering: () => false,
      isDragging: () => false,
      hitTest: () => false,
      setCentroid: () => undefined,
      setMode: () => undefined,
    } as unknown as GizmoManager;

    selection = new SceneSelection(scene, camera, renderer, gizmo);
    selection.selectionHandlers = {
      select: vi.fn(),
      clearSelection: vi.fn(),
      selectExactly: vi.fn(),
      contextMenu: vi.fn(),
    } as unknown as SceneSelectionHandlers;
    controls.setNavigationSink((active) => selection.onCameraNavigation(active));
    controls.setCancelDragSink(() => selection.cancelActiveDrag());

    // One box dead centre, so a centred press hits it and a press anywhere
    // else is empty bed.
    const box = new Mesh(new BoxGeometry(4, 4, 4), new MeshBasicMaterial());
    box.updateMatrixWorld(true);
    scene.add(box);
    selection.register('7', box);
    selection.setObjectMode('paint');
    selection.setPaintBrush('enforcer', 2);

    // The viewer's boundary: each dab is one `PaintSupport` op.
    selection.gizmoHandlers = {
      delta: () => undefined,
      end: () => undefined,
      facePicked: () => undefined,
      paintDab: () => {
        paintOps += 1;
      },
      paintEnd: () => undefined,
      paintRadiusChange: () => undefined,
    } as never;
  });

  afterEach(() => {
    selection.dispose();
    controls.dispose();
    orbit.dispose();
    canvas.remove();
  });

  function dispatch(type: string, props: Record<string, unknown>): void {
    canvas.dispatchEvent(pointerEvent(type, props));
  }

  it('an orbit that starts on empty bed and sweeps across the model emits zero PaintSupport ops', () => {
    // Press the empty corner (misses the centred box), then drag the pointer
    // straight across the model — the #373 "panning paints" symptom.
    dispatch('pointerdown', { clientX: 10, clientY: 10 });
    expect(cameraListener).toHaveBeenCalledTimes(1);

    for (const x of [20, 40, 60, 80, 100]) {
      dispatch('pointermove', { clientX: x, clientY: 10 });
    }
    dispatch('pointermove', { clientX: CENTRE, clientY: CENTRE });
    dispatch('pointerup', { clientX: CENTRE, clientY: CENTRE });

    expect(paintOps).toBe(0);
  });

  it('a middle-button pan over the model emits zero PaintSupport ops', () => {
    dispatch('pointerdown', { button: 1, ...onModel() });
    dispatch('pointermove', { button: 1, ...onModel(30) });
    dispatch('pointerup', { button: 1, ...onModel(30) });

    expect(paintOps).toBe(0);
  });

  it('a right-button pan over the model emits zero PaintSupport ops', () => {
    dispatch('pointerdown', { button: 2, ...onModel() });
    dispatch('pointermove', { button: 2, ...onModel(30) });
    dispatch('pointerup', { button: 2, ...onModel(30) });

    expect(paintOps).toBe(0);
  });

  it('a pure wheel zoom over the model emits zero PaintSupport ops', () => {
    const wheel = new Event('wheel', { bubbles: true, cancelable: true });
    Object.assign(wheel, { deltaY: -120, clientX: CENTRE, clientY: CENTRE });
    canvas.dispatchEvent(wheel);

    expect(paintOps).toBe(0);
  });

  it('a stroke that a two-finger camera gesture takes over stops emitting PaintSupport ops', () => {
    // A legitimate stroke: primary press on the model lays one dab.
    dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, ...onModel() });
    expect(paintOps).toBe(1);

    // The camera claims the gesture via a second contact.
    selection.onCameraNavigation(true);
    const before = paintOps;
    dispatch('pointermove', { pointerType: 'touch', pointerId: 1, ...onModel(20) });

    expect(paintOps).toBe(before);
  });

  // Rule 1 of the guard: a paint press on the model must be *withheld* from the
  // camera — the stroke owns the gesture, so the camera's bubble listener never
  // runs. If the claim used plain `stopPropagation` this listener would still
  // fire and the camera would orbit under the brush.
  it('rule 1: a primary press on the model silences the camera listener', () => {
    dispatch('pointerdown', { ...onModel() });

    expect(paintOps).toBe(1);
    expect(cameraListener).toHaveBeenCalledTimes(0);
  });

  // Rule 2: camera navigation taking over a live stroke discards it — its
  // remaining moves are navigation, not dabs, and its lift opens no history.
  it('rule 2: camera navigation taking over abandons the live stroke', () => {
    dispatch('pointerdown', { ...onModel() });
    expect(paintOps).toBe(1);

    selection.onCameraNavigation(true);
    const before = paintOps;
    dispatch('pointermove', { ...onModel(20) });
    dispatch('pointermove', { ...onModel(30) });

    expect(paintOps).toBe(before);
  });

  // Rule 3: while camera navigation is active, paint input is ignored entirely
  // — the press is unclaimed (the camera gets it) and no stroke is armed.
  it('rule 3: paint input is ignored while camera navigation is active', () => {
    selection.onCameraNavigation(true);
    selection.onCameraNavigation(true); // redundant reports must not flip it

    dispatch('pointerdown', { ...onModel() });
    dispatch('pointermove', { ...onModel(20) });

    expect(paintOps).toBe(0);
    expect(cameraListener).toHaveBeenCalled();

    // Navigation ended; the very next press paints again.
    selection.onCameraNavigation(false);
    selection.onCameraNavigation(false);
    dispatch('pointerup', { ...onModel(20) });
    dispatch('pointerdown', { ...onModel() });

    expect(paintOps).toBe(1);
  });
});
