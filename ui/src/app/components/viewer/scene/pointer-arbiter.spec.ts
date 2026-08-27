import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { isPalmTouch, PALM_CONTACT_MIN_PX, PEN_GRACE_MS, PointerArbiter } from './pointer-arbiter';

describe('isPalmTouch', () => {
  const base = {
    enabled: true,
    penActive: false,
    penEverUsed: false,
    contactMaxPx: 10,
    palmContactMinPx: PALM_CONTACT_MIN_PX,
  };

  it('never rejects when disabled', () => {
    expect(isPalmTouch({ ...base, enabled: false, penActive: true })).toBe(false);
    expect(isPalmTouch({ ...base, enabled: false, penEverUsed: true, contactMaxPx: 200 })).toBe(
      false,
    );
  });

  it('rejects any touch while a pen is active', () => {
    expect(isPalmTouch({ ...base, penActive: true })).toBe(true);
  });

  it('allows normal touches when no pen has ever been used', () => {
    expect(isPalmTouch({ ...base, penActive: false, penEverUsed: false })).toBe(false);
    // Even a large contact is allowed for pure-touch users.
    expect(isPalmTouch({ ...base, penEverUsed: false, contactMaxPx: 200 })).toBe(false);
  });

  it('rejects palm-sized contacts once a pen has been seen this session', () => {
    expect(isPalmTouch({ ...base, penEverUsed: true, contactMaxPx: PALM_CONTACT_MIN_PX })).toBe(
      true,
    );
    expect(isPalmTouch({ ...base, penEverUsed: true, contactMaxPx: PALM_CONTACT_MIN_PX - 1 })).toBe(
      false,
    );
  });
});

/**
 * Builds a plain Event carrying the PointerEvent fields the arbiter reads. A
 * full PointerEvent constructor is not available in every test environment, and
 * the arbiter only touches `pointerType`, `pointerId`, `width`, `height`, and
 * the standard propagation methods — all present on a base Event.
 */
function pointerEvent(type: string, props: Partial<PointerEvent>): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.assign(event, { pointerId: 1, width: 10, height: 10, ...props });
  return event as Event;
}

describe('PointerArbiter', () => {
  let host: HTMLElement;
  let canvas: HTMLElement;
  let arbiter: PointerArbiter;
  let clock: { t: number };
  let originalMaxTouchPoints: PropertyDescriptor | undefined;

  /** Dispatch on the canvas; returns true if a downstream canvas listener saw it. */
  function dispatch(type: string, props: Partial<PointerEvent>): boolean {
    let reached = false;
    const spy = (): void => {
      reached = true;
    };
    canvas.addEventListener(type, spy);
    canvas.dispatchEvent(pointerEvent(type, props));
    canvas.removeEventListener(type, spy);
    return reached;
  }

  beforeEach(() => {
    originalMaxTouchPoints = Object.getOwnPropertyDescriptor(navigator, 'maxTouchPoints');
    Object.defineProperty(navigator, 'maxTouchPoints', { value: 5, configurable: true });

    host = document.createElement('div');
    canvas = document.createElement('canvas');
    host.appendChild(canvas);
    document.body.appendChild(host);

    clock = { t: 1_000 };
    arbiter = new PointerArbiter(host, () => clock.t);
  });

  afterEach(() => {
    arbiter.dispose();
    host.remove();
    if (originalMaxTouchPoints) {
      Object.defineProperty(navigator, 'maxTouchPoints', originalMaxTouchPoints);
    }
  });

  it('lets normal finger touches through when no pen is in use', () => {
    expect(dispatch('pointerdown', { pointerType: 'touch', pointerId: 1 })).toBe(true);
  });

  it('swallows a palm touch while a pen is down', () => {
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    expect(arbiter.isPenActive()).toBe(true);
    expect(dispatch('pointerdown', { pointerType: 'touch', pointerId: 1 })).toBe(false);
  });

  it('keeps swallowing the whole life of a committed palm pointer', () => {
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    expect(dispatch('pointerdown', { pointerType: 'touch', pointerId: 1 })).toBe(false);
    // Even after the pen lifts and grace elapses, the already-committed palm
    // pointer's own move/up stream stays swallowed so it can't start a gesture.
    dispatch('pointerup', { pointerType: 'pen', pointerId: 9 });
    clock.t += PEN_GRACE_MS + 100;
    expect(dispatch('pointermove', { pointerType: 'touch', pointerId: 1 })).toBe(false);
    expect(dispatch('pointerup', { pointerType: 'touch', pointerId: 1 })).toBe(false);
  });

  it('rejects touch within the grace window after the pen lifts', () => {
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    dispatch('pointerup', { pointerType: 'pen', pointerId: 9 });
    clock.t += PEN_GRACE_MS - 50;
    expect(arbiter.isPenActive()).toBe(true);
    expect(dispatch('pointerdown', { pointerType: 'touch', pointerId: 2 })).toBe(false);
  });

  it('allows touch again once the grace window elapses', () => {
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    dispatch('pointerout', { pointerType: 'pen', pointerId: 9 });
    clock.t += PEN_GRACE_MS + 50;
    expect(arbiter.isPenActive()).toBe(false);
    expect(dispatch('pointerdown', { pointerType: 'touch', pointerId: 2 })).toBe(true);
  });

  it('rejects a palm-sized contact after the pen lifts (hover-less iPad case)', () => {
    // Pen used once, then fully gone (past grace).
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    dispatch('pointerout', { pointerType: 'pen', pointerId: 9 });
    clock.t += PEN_GRACE_MS + 500;
    expect(arbiter.isPenActive()).toBe(false);
    // A fingertip-sized contact passes…
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2, width: 20, height: 20 }),
    ).toBe(true);
    // …but a palm-sized one is rejected.
    expect(
      dispatch('pointerdown', {
        pointerType: 'touch',
        pointerId: 3,
        width: PALM_CONTACT_MIN_PX + 20,
        height: PALM_CONTACT_MIN_PX + 20,
      }),
    ).toBe(false);
  });

  it('passes everything through when disabled', () => {
    arbiter.setEnabled(false);
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    expect(dispatch('pointerdown', { pointerType: 'touch', pointerId: 1 })).toBe(true);
  });

  it('stops arbitrating after dispose', () => {
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    arbiter.dispose();
    expect(dispatch('pointerdown', { pointerType: 'touch', pointerId: 1 })).toBe(true);
  });
});
