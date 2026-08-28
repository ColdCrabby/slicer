import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  isPalmTouch,
  PALM_CONTACT_MIN_PX,
  PEN_CONTACT_STALE_MS,
  PEN_GRACE_MS,
  PEN_SIZE_ARM_MS,
  PointerArbiter,
  TOUCH_VERDICT_STALE_MS,
} from './pointer-arbiter';

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

  it('rejects a lone palm-sized contact after the pen lifts (hover-less iPad case)', () => {
    // Pen used once, then fully gone (past grace, still within the size window).
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    dispatch('pointerout', { pointerType: 'pen', pointerId: 9 });
    clock.t += PEN_GRACE_MS + 500;
    expect(arbiter.isPenActive()).toBe(false);
    // A fingertip-sized contact passes…
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2, width: 20, height: 20 }),
    ).toBe(true);
    // …lift it so the next contact opens a fresh group…
    dispatch('pointerup', { pointerType: 'touch', pointerId: 2, width: 20, height: 20 });
    // …and a lone palm-sized one is rejected.
    expect(
      dispatch('pointerdown', {
        pointerType: 'touch',
        pointerId: 3,
        width: PALM_CONTACT_MIN_PX + 20,
        height: PALM_CONTACT_MIN_PX + 20,
      }),
    ).toBe(false);
  });

  it('does not tear a two-finger gesture when the second finger is palm-sized', () => {
    // A pen has been used, arming the size heuristic; the pencil is now idle.
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    dispatch('pointerout', { pointerType: 'pen', pointerId: 9 });
    clock.t += PEN_GRACE_MS + 500;
    // First finger (fingertip) opens the group and is admitted.
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2, width: 20, height: 20 }),
    ).toBe(true);
    // Second finger lands palm-sized but INHERITS the group's admit verdict, so
    // the pair reaches the two-finger handler instead of collapsing to a lone
    // single-finger rotate.
    expect(
      dispatch('pointerdown', {
        pointerType: 'touch',
        pointerId: 3,
        width: PALM_CONTACT_MIN_PX + 20,
        height: PALM_CONTACT_MIN_PX + 20,
      }),
    ).toBe(true);
  });

  it('does not tear a two-finger gesture when a pen goes active mid-gesture', () => {
    // First finger lands with no pen in play → admitted, opens the group.
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, width: 20, height: 20 }),
    ).toBe(true);
    // A pen now goes active — a freshly-classified touch would be rejected…
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    expect(arbiter.isPenActive()).toBe(true);
    // …but the second finger inherits the group's admit verdict, so both fingers
    // still reach the two-finger handler (no single-finger-rotate spazz).
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2, width: 20, height: 20 }),
    ).toBe(true);
  });

  it('rejects both contacts of a resting hand while a pen is down', () => {
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    // The palm opens the group (pen active → palm) …
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, width: 50, height: 50 }),
    ).toBe(false);
    // … and the second wrist/palm contact inherits the palm verdict.
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2, width: 50, height: 50 }),
    ).toBe(false);
  });

  it('stops rejecting palm-sized contacts once the pen-size window elapses', () => {
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    dispatch('pointerout', { pointerType: 'pen', pointerId: 9 });
    // Pencil clearly set down; the user is on fingers now.
    clock.t += PEN_SIZE_ARM_MS + 100;
    expect(arbiter.isPenActive()).toBe(false);
    expect(
      dispatch('pointerdown', {
        pointerType: 'touch',
        pointerId: 2,
        width: PALM_CONTACT_MIN_PX + 20,
        height: PALM_CONTACT_MIN_PX + 20,
      }),
    ).toBe(true);
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

  it('never swallows a pointercancel for a non-palm id (lets the reset through)', () => {
    // `beginTwoFinger` dispatches a synthetic `pointercancel` for its admitted
    // fingers to reset OrbitControls. The arbiter swallows on up/cancel only for
    // a stored *palm* verdict, so a cancel for an untracked/admitted id passes
    // through and the reset is not defeated.
    dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, width: 20, height: 20 });
    expect(dispatch('pointercancel', { pointerType: 'touch', pointerId: 1 })).toBe(true);
    // An id that was never seen at all also passes through.
    expect(dispatch('pointercancel', { pointerType: 'touch', pointerId: 5 })).toBe(true);
  });

  it('recovers from a dropped palm-up instead of locking out all touch', () => {
    // A pen arms rejection; a palm lands and is swallowed…
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, width: 60, height: 60 }),
    ).toBe(false);
    // …but its pointerup is never delivered (OS dropped it) and the pen leaves.
    dispatch('pointerout', { pointerType: 'pen', pointerId: 9 });
    // Long enough that both the pen and the phantom palm verdict go stale.
    clock.t += Math.max(TOUCH_VERDICT_STALE_MS, PEN_SIZE_ARM_MS) + 100;
    // Without staleness pruning the leaked palm entry would make this new finger
    // inherit 'palm' and be swallowed — a total touch lockout. It is admitted.
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2, width: 20, height: 20 }),
    ).toBe(true);
  });

  it('keeps a still-but-live palm rejecting later contacts across the stale window', () => {
    // A pen arms rejection and a resting palm is swallowed…
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, width: 60, height: 60 }),
    ).toBe(false);
    dispatch('pointerout', { pointerType: 'pen', pointerId: 9 });
    // The hand shifts between strokes — the still-down palm streams a move that
    // refreshes its timestamp even though the pen is now idle.
    clock.t += TOUCH_VERDICT_STALE_MS - 100;
    expect(dispatch('pointermove', { pointerType: 'touch', pointerId: 1 })).toBe(false);
    // Enough more time that the palm would be stale from its *original* down,
    // but not from the refreshing move.
    clock.t += TOUCH_VERDICT_STALE_MS - 100;
    // A second contact from the same hand still inherits the live palm verdict
    // (it was not pruned), so it is swallowed rather than admitted.
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 2, width: 20, height: 20 }),
    ).toBe(false);
  });

  it('times out a stuck pen-contact latch so touch is not disabled forever', () => {
    // Pen goes down but its up/out is dropped (backgrounded mid-stroke).
    dispatch('pointerdown', { pointerType: 'pen', pointerId: 9 });
    expect(arbiter.isPenActive()).toBe(true);
    clock.t += PEN_CONTACT_STALE_MS + 100;
    // The latch has gone stale, so the pen no longer counts as active…
    expect(arbiter.isPenActive()).toBe(false);
    // …and ordinary finger touch works again.
    expect(
      dispatch('pointerdown', { pointerType: 'touch', pointerId: 1, width: 20, height: 20 }),
    ).toBe(true);
  });
});
