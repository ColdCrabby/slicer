import { PerspectiveCamera } from 'three';
import type { WebGLRenderer } from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';
import { SceneControls } from './controls';

/**
 * jsdom dispatches synthetic pointer events it does not track as active
 * pointers, so its `setPointerCapture` rejects them — the real canvas capture
 * behaviour is out of scope here; the navigation signal under test only needs
 * the OrbitControls event flow. Capture *state* itself must be modelled,
 * because the two-finger handoff cancels exactly the pointers OrbitControls
 * holds capture for.
 */
function stubPointerCapture(canvas: HTMLCanvasElement): void {
  const captured = new Set<number>();
  canvas.setPointerCapture = (pointerId: number) => {
    captured.add(pointerId);
  };
  canvas.releasePointerCapture = (pointerId: number) => {
    captured.delete(pointerId);
  };
  canvas.hasPointerCapture = (pointerId: number) => captured.has(pointerId);
}

/** A PointerEvent-shaped event; jsdom has no usable `PointerEvent` constructor. */
function pointerEvent(type: string, props: Record<string, unknown>): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.assign(event, { pointerId: 1, pointerType: 'mouse', button: 0, ...props });
  return event;
}

describe('SceneControls navigation signal', () => {
  let canvas: HTMLCanvasElement;
  let camera: PerspectiveCamera;
  let controls: OrbitControls;
  let sceneControls: SceneControls;
  let navigation: Mock<(active: boolean) => void>;

  beforeEach(() => {
    canvas = document.createElement('canvas');
    document.body.appendChild(canvas);
    stubPointerCapture(canvas);

    camera = new PerspectiveCamera(50, 1, 0.1, 1000);
    camera.position.set(0, 0, 10);
    camera.updateMatrixWorld(true);
    controls = new OrbitControls(camera, canvas);

    sceneControls = new SceneControls(camera, controls, {
      domElement: canvas,
    } as unknown as WebGLRenderer);
    navigation = vi.fn();
    sceneControls.setNavigationSink(navigation);
  });

  afterEach(() => {
    sceneControls.dispose();
    controls.dispose();
    canvas.remove();
  });

  it('brackets an orbit drag with navigation start and end', () => {
    canvas.dispatchEvent(pointerEvent('pointerdown', { clientX: 10, clientY: 10 }));
    expect(navigation).toHaveBeenLastCalledWith(true);

    canvas.dispatchEvent(pointerEvent('pointerup', { clientX: 12, clientY: 10 }));
    expect(navigation).toHaveBeenLastCalledWith(false);
  });

  it('reports middle-button autoscroll as navigation', () => {
    canvas.dispatchEvent(pointerEvent('pointerdown', { button: 1, clientX: 10, clientY: 10 }));
    expect(navigation).toHaveBeenLastCalledWith(true);

    canvas.dispatchEvent(pointerEvent('pointerup', { button: 1, clientX: 10, clientY: 8 }));
    expect(navigation).toHaveBeenLastCalledWith(false);
  });

  it('reports a two-finger gesture as navigation, through the orbit handoff', () => {
    // First contact starts an orbit drag in OrbitControls — navigation on.
    canvas.dispatchEvent(
      pointerEvent('pointerdown', { pointerType: 'touch', clientX: 10, clientY: 100 }),
    );
    expect(navigation).toHaveBeenLastCalledWith(true);

    // Second contact hands the gesture to the custom two-finger controller.
    // The synthetic pointercancel to OrbitControls ends its drag, but the
    // two-finger gesture is still navigating, so the combined signal must not
    // dip to false in between.
    canvas.dispatchEvent(
      pointerEvent('pointerdown', {
        pointerType: 'touch',
        pointerId: 2,
        clientX: 30,
        clientY: 100,
      }),
    );
    expect(navigation).toHaveBeenLastCalledWith(true);

    canvas.dispatchEvent(
      pointerEvent('pointerup', { pointerType: 'touch', pointerId: 2, clientX: 30, clientY: 96 }),
    );
    canvas.dispatchEvent(
      pointerEvent('pointerup', { pointerType: 'touch', pointerId: 1, clientX: 10, clientY: 100 }),
    );
    expect(navigation).toHaveBeenLastCalledWith(false);
    expect(navigation).toHaveBeenCalledWith(true);
  });
});
