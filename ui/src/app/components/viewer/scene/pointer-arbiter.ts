/**
 * Pen-priority palm rejection ("wrist detection") for the 3D viewport.
 *
 * On an iPad — or any screen that mixes a stylus with finger touch — the hand
 * resting on the glass while drawing with an Apple Pencil fires
 * `pointerType === 'touch'` events for the palm and wrist. Left unfiltered,
 * those stray contacts drive the camera: OrbitControls' single-touch rotate
 * spins the view, and two palm contacts read as a pinch/pan/roll gesture, so
 * the model lurches while the user is simply trying to work with the pencil.
 * That is the "clunky, weird pen support" this arbiter fixes.
 *
 * It watches every pointer event on the viewport **before** any camera,
 * selection, or gizmo handler by listening in the *capture* phase on the
 * canvas's `host` element (an ancestor of the WebGL canvas). Capture on an
 * ancestor is guaranteed by the DOM to run ahead of every listener on the
 * canvas itself — including OrbitControls' — so a palm contact can be swallowed
 * with {@link Event.stopImmediatePropagation} before anything downstream sees
 * it. Because the palm's `pointerdown` never reaches OrbitControls, no camera
 * motion is ever started and there is no half-finished gesture state to unwind.
 *
 * A touch is classified as palm at its `pointerdown` (see {@link isPalmTouch})
 * when either:
 *
 *   - a pen is currently down or hovering, or was lifted within the grace
 *     window ({@link PEN_GRACE_MS}). Hover-capable Pencils (M2 iPad Pro +
 *     Pencil Pro) fire hover events before the tip lands, so this pre-arms
 *     rejection and the palm is rejected seamlessly; or
 *   - the pen has been used at least once this session **and** the contact
 *     patch is palm-sized ({@link PALM_CONTACT_MIN_PX}). This catches the palm
 *     that lands a moment before the tip on iPads without pencil hover.
 *
 * Pure-touch users are never affected: the contact-size path is gated behind
 * "a pen has been seen this session", and the pen-active path only fires while
 * a pen is actually in use. Genuine two-finger orbit/pinch/pan therefore keeps
 * working exactly as before whenever no pen is involved.
 */

/**
 * How long after a pen lifts off the glass touch input stays suppressed.
 * Covers the beat between finishing a pencil stroke and lifting the palm —
 * and a quick tip re-plant — without stranding two-finger gestures for long.
 */
export const PEN_GRACE_MS = 600;

/**
 * Contact patch size (max of `PointerEvent.width`/`height`, CSS px) at or above
 * which a touch is treated as a palm/wrist rather than a fingertip. iPad Safari
 * reports genuine contact geometry: fingertips land around 15–35 px, while a
 * resting palm edge is markedly larger. Only consulted once a pen has been seen
 * this session, so ordinary fat-finger touches on pen-less devices are safe.
 */
export const PALM_CONTACT_MIN_PX = 45;

/**
 * The minimal, side-effect-free inputs needed to decide whether a touch is a
 * palm. Extracted so the decision is unit-testable without a DOM.
 */
export interface PalmRejectionState {
  /** Master switch — when `false` every touch is allowed through. */
  enabled: boolean;
  /** A pen is down, hovering, or was active within {@link PEN_GRACE_MS}. */
  penActive: boolean;
  /** A pen has produced at least one event this session. */
  penEverUsed: boolean;
  /** `max(width, height)` of this touch's contact patch, in CSS px. */
  contactMaxPx: number;
  /** Threshold above which {@link contactMaxPx} counts as a palm. */
  palmContactMinPx: number;
}

/**
 * Pure predicate at the heart of palm rejection. Returns `true` when a touch
 * pointer should be swallowed as a palm/wrist contact. See the module comment
 * for the rationale behind each branch.
 */
export function isPalmTouch(state: PalmRejectionState): boolean {
  if (!state.enabled) {
    return false;
  }
  // A pen is in play — every concurrent touch is the supporting hand.
  if (state.penActive) {
    return true;
  }
  // No pen right now, but the user has a pen and this contact is palm-sized:
  // the hand that landed a beat before the tip on a hover-less iPad.
  if (state.penEverUsed && state.contactMaxPx >= state.palmContactMinPx) {
    return true;
  }
  return false;
}

/**
 * Installs pen-priority palm rejection on a viewport host element and tracks
 * pen presence over time. One instance per {@link ViewerScene}.
 */
export class PointerArbiter {
  private enabled = true;
  private penEverUsed = false;
  /** A pen is currently down or hovering (distinct from the grace window). */
  private penContact = false;
  /** `performance.now()` of the most recent pen event, for the grace window. */
  private lastPenActivityMs = Number.NEGATIVE_INFINITY;
  /** Touch pointer ids currently classified as palm — swallowed for their life. */
  private readonly palmPointerIds = new Set<number>();

  private readonly listenerOptions: AddEventListenerOptions = { capture: true };

  /**
   * @param host  The viewport host element (ancestor of the WebGL canvas).
   * @param now   Monotonic clock in ms; injectable for deterministic tests.
   *              Defaults to `performance.now()` (falling back to `Date.now()`).
   */
  constructor(
    private readonly host: HTMLElement,
    private readonly now: () => number = () =>
      typeof performance !== 'undefined' ? performance.now() : Date.now(),
  ) {
    // Pen-less desktops never emit touch/pen events, so skip the per-move
    // capture listeners entirely there. iPads, Surface tablets, and 2-in-1s
    // all report a positive touch-point count.
    const touchCapable = typeof navigator !== 'undefined' && (navigator.maxTouchPoints ?? 0) > 0;
    if (!touchCapable) {
      return;
    }
    host.addEventListener('pointerdown', this.onPointerDown, this.listenerOptions);
    host.addEventListener('pointermove', this.onPointerMove, this.listenerOptions);
    host.addEventListener('pointerup', this.onPointerUp, this.listenerOptions);
    host.addEventListener('pointercancel', this.onPointerUp, this.listenerOptions);
    host.addEventListener('pointerout', this.onPointerOut, this.listenerOptions);
  }

  /** Enable or disable palm rejection (a user preference). Default on. */
  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (!enabled) {
      this.palmPointerIds.clear();
    }
  }

  dispose(): void {
    this.host.removeEventListener('pointerdown', this.onPointerDown, this.listenerOptions);
    this.host.removeEventListener('pointermove', this.onPointerMove, this.listenerOptions);
    this.host.removeEventListener('pointerup', this.onPointerUp, this.listenerOptions);
    this.host.removeEventListener('pointercancel', this.onPointerUp, this.listenerOptions);
    this.host.removeEventListener('pointerout', this.onPointerOut, this.listenerOptions);
    this.palmPointerIds.clear();
  }

  /**
   * True while a pen is down, hovering, or was active within the grace window.
   * Exposed for diagnostics and potential UI affordances.
   */
  isPenActive(): boolean {
    if (this.penContact) {
      return true;
    }
    return this.now() - this.lastPenActivityMs < PEN_GRACE_MS;
  }

  private notePenActivity(): void {
    this.penEverUsed = true;
    this.penContact = true;
    this.lastPenActivityMs = this.now();
  }

  private swallow(event: PointerEvent): void {
    event.stopImmediatePropagation();
    event.stopPropagation();
    if (event.cancelable) {
      event.preventDefault();
    }
  }

  private classify(event: PointerEvent): boolean {
    return isPalmTouch({
      enabled: this.enabled,
      penActive: this.isPenActive(),
      penEverUsed: this.penEverUsed,
      contactMaxPx: Math.max(event.width ?? 0, event.height ?? 0),
      palmContactMinPx: PALM_CONTACT_MIN_PX,
    });
  }

  private readonly onPointerDown = (event: PointerEvent): void => {
    if (event.pointerType === 'pen') {
      this.notePenActivity();
      return;
    }
    if (event.pointerType !== 'touch') {
      return;
    }
    if (this.classify(event)) {
      this.palmPointerIds.add(event.pointerId);
      this.swallow(event);
    }
  };

  private readonly onPointerMove = (event: PointerEvent): void => {
    if (event.pointerType === 'pen') {
      this.notePenActivity();
      return;
    }
    if (event.pointerType !== 'touch') {
      return;
    }
    // A touch already committed to "palm" stays swallowed for its whole life so
    // its move stream can never leak into a gesture handler.
    if (this.palmPointerIds.has(event.pointerId)) {
      this.swallow(event);
    }
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    if (event.pointerType === 'pen') {
      this.penContact = false;
      this.lastPenActivityMs = this.now();
      return;
    }
    if (event.pointerType !== 'touch') {
      return;
    }
    if (this.palmPointerIds.delete(event.pointerId)) {
      this.swallow(event);
    }
  };

  private readonly onPointerOut = (event: PointerEvent): void => {
    // Pen left hover range (or the surface). Drop the "in contact" flag so the
    // grace window starts counting down; touch resumes once it elapses.
    if (event.pointerType === 'pen') {
      this.penContact = false;
      this.lastPenActivityMs = this.now();
    }
  };
}
