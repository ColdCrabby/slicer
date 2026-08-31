import { BoxGeometry, Mesh, MeshBasicMaterial, PerspectiveCamera, Scene } from 'three';
import type { WebGLRenderer } from 'three';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { GizmoManager } from '../gizmo';
import { SceneSelection } from './selection';
import type { SceneSelectionHandlers } from './types';

const CANVAS_SIZE = 200;
/** Client coordinates of the canvas centre, where the test object sits. */
const CENTRE = CANVAS_SIZE / 2;

/**
 * A plain Event carrying the PointerEvent fields the selection reads. jsdom
 * does not offer a usable `PointerEvent` constructor, and only `pointerId`,
 * `pointerType`, `button` and the client coordinates are ever consulted.
 */
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

describe('SceneSelection', () => {
  let canvas: HTMLCanvasElement;
  let selection: SceneSelection;
  /**
   * Stands in for OrbitControls: a bubble-phase `pointerdown` listener on the
   * same canvas. Whether it runs is exactly the question of whether the camera
   * gets to act on a press.
   */
  let cameraListener: ReturnType<typeof vi.fn>;
  let cameraLiftListener: ReturnType<typeof vi.fn>;
  let handlers: {
    select: ReturnType<typeof vi.fn>;
    clearSelection: ReturnType<typeof vi.fn>;
    contextMenu: ReturnType<typeof vi.fn>;
  };

  /** Dispatch a pointer event on the canvas the selection listens to. */
  function dispatch(type: string, props: Record<string, unknown> = {}): void {
    canvas.dispatchEvent(pointerEvent(type, props));
  }

  /** Press, drift by `driftPx`, and lift — the gesture a tap has to survive. */
  function tap(pointerType: string, driftPx: number, props: Record<string, unknown> = {}): void {
    dispatch('pointerdown', { pointerType, clientX: CENTRE, clientY: CENTRE, ...props });
    if (driftPx > 0) {
      dispatch('pointermove', { pointerType, clientX: CENTRE + driftPx, clientY: CENTRE });
    }
    dispatch('pointerup', { pointerType, clientX: CENTRE + driftPx, clientY: CENTRE });
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

    // Registered before `SceneSelection` and without capture, exactly as
    // OrbitControls does it, so the phase ordering under test is the real one.
    // The lift matters as much as the press: OrbitControls tracks the pointer
    // from `pointerdown` and only lets go on `pointerup`.
    cameraListener = vi.fn();
    cameraLiftListener = vi.fn();
    canvas.addEventListener('pointerdown', cameraListener);
    canvas.addEventListener('pointerup', cameraLiftListener);

    const scene = new Scene();
    const camera = new PerspectiveCamera(50, 1, 0.1, 1000);
    camera.position.set(0, 0, 10);
    camera.updateMatrixWorld(true);

    const gizmo = {
      isHovering: () => false,
      isDragging: () => false,
      hitTest: () => false,
      setCentroid: () => undefined,
      setMode: () => undefined,
    } as unknown as GizmoManager;

    selection = new SceneSelection(
      scene,
      camera,
      { domElement: canvas } as unknown as WebGLRenderer,
      gizmo,
    );

    // One box dead centre of the view, so a press at the canvas centre hits it.
    const box = new Mesh(new BoxGeometry(4, 4, 4), new MeshBasicMaterial());
    box.updateMatrixWorld(true);
    scene.add(box);
    selection.register('7', box);

    handlers = {
      select: vi.fn(),
      clearSelection: vi.fn(),
      contextMenu: vi.fn(),
    };
    selection.selectionHandlers = handlers as unknown as SceneSelectionHandlers;
  });

  afterEach(() => {
    selection.dispose();
    canvas.remove();
    vi.useRealTimers();
  });

  describe('tap tolerance', () => {
    it('selects on a still press from every pointer type', () => {
      for (const pointerType of ['mouse', 'pen', 'touch']) {
        handlers.select.mockClear();
        tap(pointerType, 0);
        expect(handlers.select, pointerType).toHaveBeenCalledWith('7', false);
      }
    });

    // The regression this tolerance exists for: a fingertip never lands and
    // lifts on the same pixel, so a mouse-sized threshold rejected real taps
    // and left the objects list as the only way to select anything.
    it('still selects when a finger drifts by more than a mouse would', () => {
      tap('touch', 12);
      expect(handlers.select).toHaveBeenCalledWith('7', false);
    });

    it('still selects when a stylus wobbles', () => {
      tap('pen', 7);
      expect(handlers.select).toHaveBeenCalledWith('7', false);
    });

    it('does not select when the pointer travels far enough to be a drag', () => {
      tap('touch', 40);
      expect(handlers.select).not.toHaveBeenCalled();
    });

    it('keeps a mouse drag from selecting, so it can orbit', () => {
      tap('mouse', 10);
      expect(handlers.select).not.toHaveBeenCalled();
    });
  });

  describe('selection', () => {
    it('clears the selection when empty space is tapped', () => {
      selection.setSelectedIds(new Set(['7']));
      handlers.select.mockClear();
      // Well outside the object, but still over the canvas.
      dispatch('pointerdown', { pointerType: 'touch', clientX: 4, clientY: 4 });
      dispatch('pointerup', { pointerType: 'touch', clientX: 4, clientY: 4 });
      expect(handlers.clearSelection).toHaveBeenCalled();
    });

    it('adds to the selection when a modifier is held', () => {
      tap('mouse', 0, { shiftKey: true });
      expect(handlers.select).toHaveBeenCalledWith('7', true);
    });

    // A press the camera was let into must have its lift let through too.
    // OrbitControls tracks the pointer from `pointerdown` and only releases it
    // on `pointerup`, which it listens for on the *document* — so swallowing
    // the lift strands it mid-gesture, leaving a phantom pointer and a `state`
    // that never returns to NONE. A later button-less move then orbits the
    // view, and on touch the next single finger reads as a second contact.
    it('lets the camera see the lift of every press it saw', () => {
      for (const pointerType of ['mouse', 'pen', 'touch']) {
        cameraListener.mockClear();
        cameraLiftListener.mockClear();
        tap(pointerType, 0);
        expect(cameraListener, pointerType).toHaveBeenCalled();
        expect(cameraLiftListener, pointerType).toHaveBeenCalled();
      }
    });

    it('lets the camera see the lift of a face pick too', () => {
      selection.gizmoHandlers = { delta: vi.fn(), end: vi.fn(), facePicked: vi.fn() };
      selection.setObjectMode('pullToFloor');
      tap('mouse', 0);
      expect(cameraLiftListener).toHaveBeenCalled();
    });

    // Touch has no modifier key, so the toggle is the only way to build a
    // multi-object selection without going to the objects list.
    it('adds to the selection when multi-select is on, with no modifier', () => {
      selection.setAdditiveSelection(true);
      tap('touch', 0);
      expect(handlers.select).toHaveBeenCalledWith('7', true);
    });
  });

  describe('context menu', () => {
    it('opens on a touch long-press, and the lift does not also select', () => {
      vi.useFakeTimers();
      dispatch('pointerdown', { pointerType: 'touch' });
      vi.advanceTimersByTime(600);
      expect(handlers.contextMenu).toHaveBeenCalledWith('7', expect.anything());

      dispatch('pointerup', { pointerType: 'touch' });
      expect(handlers.select).not.toHaveBeenCalled();
    });

    it('does not open when the press moves away first', () => {
      vi.useFakeTimers();
      dispatch('pointerdown', { pointerType: 'touch' });
      dispatch('pointermove', { pointerType: 'touch', clientX: CENTRE + 60, clientY: CENTRE });
      vi.advanceTimersByTime(600);
      expect(handlers.contextMenu).not.toHaveBeenCalled();
    });

    it('reports empty space as no object', () => {
      vi.useFakeTimers();
      dispatch('pointerdown', { pointerType: 'touch', clientX: 4, clientY: 4 });
      vi.advanceTimersByTime(600);
      expect(handlers.contextMenu).toHaveBeenCalledWith(null, expect.anything());
    });

    it('opens on a right click', () => {
      dispatch('pointerdown', { button: 2 });
      dispatch('pointerup', { button: 2 });
      expect(handlers.contextMenu).toHaveBeenCalledWith('7', expect.anything());
    });

    // Right-drag pans the camera; a menu on release would fire on every pan.
    it('stays shut for a right drag', () => {
      dispatch('pointerdown', { button: 2 });
      dispatch('pointermove', { clientX: CENTRE + 60, clientY: CENTRE });
      dispatch('pointerup', { button: 2, clientX: CENTRE + 60, clientY: CENTRE });
      expect(handlers.contextMenu).not.toHaveBeenCalled();
    });

    it('is never asked for when no handler is wired', () => {
      vi.useFakeTimers();
      selection.selectionHandlers = {
        select: handlers.select,
        clearSelection: handlers.clearSelection,
      };
      dispatch('pointerdown', { pointerType: 'touch' });
      vi.advanceTimersByTime(600);
      dispatch('pointerup', { pointerType: 'touch' });
      expect(handlers.select).toHaveBeenCalledWith('7', false);
    });
  });

  describe('direct drag', () => {
    it('slides the selection when a finger drags an already-selected object', () => {
      const gizmoHandlers = { delta: vi.fn(), end: vi.fn(), facePicked: vi.fn() };
      selection.gizmoHandlers = gizmoHandlers;
      selection.setDirectDragEnabled(true);
      selection.setObjectMode('translate');
      selection.setSelectedIds(new Set(['7']));

      dispatch('pointerdown', { pointerType: 'touch' });
      dispatch('pointermove', { pointerType: 'touch', clientX: CENTRE + 40, clientY: CENTRE });
      dispatch('pointermove', { pointerType: 'touch', clientX: CENTRE + 60, clientY: CENTRE });
      dispatch('pointerup', { pointerType: 'touch', clientX: CENTRE + 60, clientY: CENTRE });

      expect(gizmoHandlers.delta).toHaveBeenCalled();
      const [ids, delta] = gizmoHandlers.delta.mock.calls[0];
      expect(ids).toEqual(['7']);
      expect(delta.kind).toBe('translate');
      // Rightwards on screen is +X in world, and the bed height never changes.
      expect(delta.delta[0]).toBeGreaterThan(0);
      expect(delta.delta[2]).toBe(0);
      expect(gizmoHandlers.end).toHaveBeenCalled();
      // A drag is not a tap.
      expect(handlers.select).not.toHaveBeenCalled();
    });

    it('leaves an unselected object alone, so the drag can orbit', () => {
      const gizmoHandlers = { delta: vi.fn(), end: vi.fn(), facePicked: vi.fn() };
      selection.gizmoHandlers = gizmoHandlers;
      selection.setDirectDragEnabled(true);
      selection.setObjectMode('translate');

      dispatch('pointerdown', { pointerType: 'touch' });
      dispatch('pointermove', { pointerType: 'touch', clientX: CENTRE + 60, clientY: CENTRE });
      dispatch('pointerup', { pointerType: 'touch', clientX: CENTRE + 60, clientY: CENTRE });

      expect(gizmoHandlers.delta).not.toHaveBeenCalled();
      // …and the camera really did get the press, rather than the drag simply
      // going nowhere. `cameraListener` is a bubble-phase listener, the shape
      // OrbitControls has on this same canvas.
      expect(cameraListener).toHaveBeenCalled();
    });

    it('never takes a mouse drag away from the camera', () => {
      const gizmoHandlers = { delta: vi.fn(), end: vi.fn(), facePicked: vi.fn() };
      selection.gizmoHandlers = gizmoHandlers;
      selection.setDirectDragEnabled(true);
      selection.setObjectMode('translate');
      selection.setSelectedIds(new Set(['7']));

      dispatch('pointerdown', { pointerType: 'mouse' });
      dispatch('pointermove', { pointerType: 'mouse', clientX: CENTRE + 60, clientY: CENTRE });
      dispatch('pointerup', { pointerType: 'mouse', clientX: CENTRE + 60, clientY: CENTRE });

      expect(gizmoHandlers.delta).not.toHaveBeenCalled();
      expect(cameraListener).toHaveBeenCalled();
    });

    // The one invariant the whole drag rests on. The camera is shut out at
    // `pointerdown`, before it is known whether the press will travel, so that
    // it never starts a rotate there is nothing left to wrestle it away from.
    // If this stops holding, the view spins under the object being dragged.
    it('shuts the camera out of a press it is about to claim', () => {
      selection.gizmoHandlers = { delta: vi.fn(), end: vi.fn(), facePicked: vi.fn() };
      selection.setDirectDragEnabled(true);
      selection.setObjectMode('translate');
      selection.setSelectedIds(new Set(['7']));

      dispatch('pointerdown', { pointerType: 'touch' });

      expect(cameraListener).not.toHaveBeenCalled();
    });

    it('ignores a second finger rather than dropping the drag', () => {
      const gizmoHandlers = { delta: vi.fn(), end: vi.fn(), facePicked: vi.fn() };
      selection.gizmoHandlers = gizmoHandlers;
      selection.setDirectDragEnabled(true);
      selection.setObjectMode('translate');
      selection.setSelectedIds(new Set(['7']));

      dispatch('pointerdown', { pointerType: 'touch', pointerId: 1 });
      dispatch('pointermove', { pointerType: 'touch', pointerId: 1, clientX: CENTRE + 40 });
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2, clientX: 20, clientY: 20 });
      gizmoHandlers.delta.mockClear();
      dispatch('pointermove', { pointerType: 'touch', pointerId: 1, clientX: CENTRE + 70 });

      expect(gizmoHandlers.delta).toHaveBeenCalled();
    });
  });

  // A palm can land a beat before the tip on an iPad without pencil hover, and
  // the arbiter only rejects it by size once a pen has been seen recently. If
  // that admitted palm held the press slot, every Pencil tap after it would be
  // discarded until the hand lifted.
  describe('pen priority', () => {
    it('lets a pen take the press over from a resting finger', () => {
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, clientX: 4, clientY: 4 });
      dispatch('pointerdown', { pointerType: 'pen', pointerId: 2 });
      dispatch('pointerup', { pointerType: 'pen', pointerId: 2 });

      expect(handlers.select).toHaveBeenCalledWith('7', false);
    });

    it('still ignores a second finger', () => {
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, clientX: 4, clientY: 4 });
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2 });
      dispatch('pointerup', { pointerType: 'touch', pointerId: 2 });

      expect(handlers.select).not.toHaveBeenCalled();
    });
  });
});
