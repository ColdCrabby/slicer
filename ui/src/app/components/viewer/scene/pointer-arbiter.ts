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
 * Pure-touch users are never affected: the contact-size path is gated behind a
 * pen having been used *recently* ({@link PEN_SIZE_ARM_MS}), and the pen-active
 * path only fires while a pen is actually in use. Genuine two-finger
 * orbit/pinch/pan therefore keeps working exactly as before whenever no pen is
 * involved.
 *
 * **No-tear invariant.** Palm rejection decides per *gesture group*, not per
 * isolated pointer. The camera's two-finger handler only engages while two
 * touches are down; a lone touch drives OrbitControls' single-finger rotate. So
 * if the arbiter ever swallowed exactly one finger of a two-finger gesture, the
 * surviving finger would spin the camera — the "spazzing" a stylus user sees
 * when a palm-sized fingertip, or a flickering pen hover/grace state, splits the
 * pair. To prevent that, a touch that lands while **exactly one** other touch is
 * down inherits that group's verdict (admit wins over palm), so a pair is
 * admitted or rejected as a whole and never split.
 *
 * **The inheritance stops at the pair.** Once two touches are already down, a
 * further contact cannot possibly split them — the pair is complete and driving
 * the camera — so a third or later touch is classified from scratch instead of
 * inheriting `'admit'`. This is what keeps a palm from *joining* a live pinch:
 * the two-finger controller re-anchors onto whichever contacts remain when a
 * finger lifts, so an admitted palm becomes half of the gesture the moment a
 * real finger leaves, and the camera lurches with the wandering contact patch.
 * Inheriting only across the 1 → 2 step gets the no-tear guarantee without that
 * hole.
 *
 * **Synthetic events are ignored.** The two-finger controller dispatches a
 * `pointercancel` per live finger to reset OrbitControls' internal drag state.
 * Those events travel the host's capture phase like real ones, and taking them
 * at face value would empty the live set while both fingers are still down —
 * destroying the group coherence above at the exact moment it matters most.
 * They carry a marker ({@link isSyntheticPointerEvent}) and are skipped.
 *
 * **Self-healing live set.** Because a mid-group contact inherits the group
 * verdict, a leaked `'palm'` entry (an iPad that drops a `pointerup`/
 * `pointercancel`) would otherwise be inherited forever and lock out all touch.
 * Two guards prevent that: touch verdicts whose pointer has produced no event
 * within {@link TOUCH_VERDICT_STALE_MS} are pruned before each new
 * classification, and the "pen is down/hovering" latch is bounded by
 * {@link PEN_CONTACT_STALE_MS}. A pointer really still down keeps refreshing its
 * timestamp, so only phantom contacts are reclaimed.
 */

import { isSyntheticPointerEvent } from './synthetic-pointer';

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
 * Once a pen has been used this session, the palm-by-size heuristic
 * ({@link PALM_CONTACT_MIN_PX}) stays armed only for this long after the pen's
 * most recent activity. Beyond it we assume the pencil has been set down and the
 * user is navigating with fingers again, so firm fingertips are no longer
 * mistaken for a palm and a stray large single-finger contact stops silently
 * failing. A palm that never lifts keeps its original verdict via the
 * group-coherence rule regardless of this window, so an actively-drawing hand is
 * still rejected across long pauses between strokes.
 */
export const PEN_SIZE_ARM_MS = 1500;

/**
 * Upper bound on how long the "a pen is physically down or hovering"
 * ({@link PointerArbiter.isPenActive}) latch is trusted without a fresh pen
 * event. The latch is set on every pen event and normally cleared on the pen's
 * `pointerup`/`pointerout`. But iOS/WebKit can *drop* that lift (app
 * backgrounded mid-stroke, cancel storms), which would otherwise pin the latch
 * on and suppress **all** touch until a reload. An actively used pen streams
 * `pointermove`s that keep the latch fresh, so this only trips after a genuine
 * idle — at which point we assume the pencil was parked and let touch resume.
 */
export const PEN_CONTACT_STALE_MS = 3000;

/**
 * How long a live touch verdict survives without any event for its pointer
 * before it is pruned. Verdicts are normally removed on `pointerup`/
 * `pointercancel`; this reconciles the set when that lift is *dropped* by the
 * OS. Without it, a leaked `'palm'` entry would be inherited by every later
 * contact (see the no-tear rule) and lock out all touch indefinitely. A contact
 * that is really still down streams `pointermove`s that refresh its timestamp,
 * so only a phantom (already-lifted) contact goes stale and is reclaimed.
 */
export const TOUCH_VERDICT_STALE_MS = 3000;

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
 * Number of live touches after which a new contact stops inheriting the group
 * verdict and is classified on its own merits. Inheritance exists purely to
 * keep a *pair* from being split (see the module doc); once a pair is down,
 * rejecting a newcomer cannot split anything, so palms are filtered again.
 */
const GROUP_INHERITANCE_MAX_LIVE_TOUCHES = 1;

/** Whether a live touch pointer is being passed through or swallowed. */
type TouchVerdict = 'admit' | 'palm';

/** A live touch pointer's verdict plus the clock of its most recent event. */
interface TouchRecord {
  verdict: TouchVerdict;
  /** `now()` of this pointer's last event — used to prune dropped contacts. */
  lastSeenMs: number;
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
  /**
   * Verdict for every touch pointer currently down — `'admit'` (passed through)
   * or `'palm'` (swallowed for its whole life), each stamped with the time of
   * its last event. Entries are added at `pointerdown`, refreshed on
   * `pointermove`, removed at `pointerup`/`pointercancel`, and reclaimed by
   * staleness if their lift is ever dropped ({@link TOUCH_VERDICT_STALE_MS}).
   * Tracking the whole live set is what lets the arbiter honour the no-tear
   * invariant: a touch that lands mid-group inherits the group's verdict instead
   * of being reclassified in isolation (which could split a two-finger gesture).
   * See the module doc.
   */
  private readonly touchVerdicts = new Map<number, TouchRecord>();

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
      this.touchVerdicts.clear();
    }
  }

  dispose(): void {
    this.host.removeEventListener('pointerdown', this.onPointerDown, this.listenerOptions);
    this.host.removeEventListener('pointermove', this.onPointerMove, this.listenerOptions);
    this.host.removeEventListener('pointerup', this.onPointerUp, this.listenerOptions);
    this.host.removeEventListener('pointercancel', this.onPointerUp, this.listenerOptions);
    this.host.removeEventListener('pointerout', this.onPointerOut, this.listenerOptions);
    this.touchVerdicts.clear();
  }

  /**
   * True while a pen is down, hovering, or was active within the grace window.
   * Exposed for diagnostics and potential UI affordances.
   */
  isPenActive(): boolean {
    const idleMs = this.now() - this.lastPenActivityMs;
    // The `penContact` latch normally clears on the pen's up/out; bound it by
    // staleness so a dropped lift can't suppress touch forever. A pen in real
    // use streams moves that keep `lastPenActivityMs` fresh.
    if (this.penContact && idleMs < PEN_CONTACT_STALE_MS) {
      return true;
    }
    return idleMs < PEN_GRACE_MS;
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

  /**
   * True while the palm-by-size heuristic is armed: a pen has been used and its
   * last activity was within {@link PEN_SIZE_ARM_MS}. Outside that window we no
   * longer second-guess firm fingertips by contact size (see the constant).
   */
  private isSizeHeuristicArmed(): boolean {
    return this.penEverUsed && this.now() - this.lastPenActivityMs < PEN_SIZE_ARM_MS;
  }

  /**
   * Classify a *fresh* touch — the first contact of a group — in isolation.
   * Later contacts never call this; they inherit the group verdict. See
   * {@link verdictForNewTouch}.
   */
  private classifyFresh(event: PointerEvent): boolean {
    return isPalmTouch({
      enabled: this.enabled,
      penActive: this.isPenActive(),
      // The size path is additionally gated on recency; `isPalmTouch` only needs
      // to know whether that path is live for this contact right now.
      penEverUsed: this.isSizeHeuristicArmed(),
      contactMaxPx: Math.max(event.width ?? 0, event.height ?? 0),
      palmContactMinPx: PALM_CONTACT_MIN_PX,
    });
  }

  /**
   * Drop verdicts whose pointer has produced no event within
   * {@link TOUCH_VERDICT_STALE_MS} — a contact whose `pointerup`/`pointercancel`
   * the OS never delivered. Keeps the live set matching physical reality so a
   * phantom `'palm'` can't be inherited by (and thus lock out) later contacts.
   */
  private pruneStaleTouches(): void {
    const cutoff = this.now() - TOUCH_VERDICT_STALE_MS;
    for (const [id, record] of this.touchVerdicts) {
      if (record.lastSeenMs < cutoff) {
        this.touchVerdicts.delete(id);
      }
    }
  }

  /**
   * The verdict a newly-pressed touch should take, honouring the no-tear
   * invariant: the first contact of a fresh group is classified from scratch;
   * any touch landing while another is already down inherits the group verdict.
   * Admit wins over palm so a genuine multi-finger gesture is never split into a
   * lone survivor that OrbitControls would read as a single-finger rotate.
   */
  private verdictForNewTouch(event: PointerEvent): TouchVerdict {
    if (!this.enabled) {
      return 'admit';
    }
    this.pruneStaleTouches();
    // Inherit only while a pair is still forming. Beyond that, a newcomer is a
    // third contact that cannot split anything — classify it properly so a palm
    // never joins (and later becomes half of) a live gesture.
    if (
      this.touchVerdicts.size > 0 &&
      this.touchVerdicts.size <= GROUP_INHERITANCE_MAX_LIVE_TOUCHES
    ) {
      for (const record of this.touchVerdicts.values()) {
        if (record.verdict === 'admit') {
          return 'admit';
        }
      }
      return 'palm';
    }
    return this.classifyFresh(event) ? 'palm' : 'admit';
  }

  private readonly onPointerDown = (event: PointerEvent): void => {
    if (isSyntheticPointerEvent(event)) {
      return;
    }
    if (event.pointerType === 'pen') {
      this.notePenActivity();
      return;
    }
    if (event.pointerType !== 'touch') {
      return;
    }
    const verdict = this.verdictForNewTouch(event);
    this.touchVerdicts.set(event.pointerId, { verdict, lastSeenMs: this.now() });
    if (verdict === 'palm') {
      this.swallow(event);
    }
  };

  private readonly onPointerMove = (event: PointerEvent): void => {
    if (isSyntheticPointerEvent(event)) {
      return;
    }
    if (event.pointerType === 'pen') {
      this.notePenActivity();
      return;
    }
    if (event.pointerType !== 'touch') {
      return;
    }
    const record = this.touchVerdicts.get(event.pointerId);
    if (!record) {
      return;
    }
    // Keep the contact fresh so staleness pruning only reclaims phantom (dropped)
    // pointers, never a finger that is really still down.
    record.lastSeenMs = this.now();
    // A touch committed to "palm" stays swallowed for its whole life so its move
    // stream can never leak into a gesture handler.
    if (record.verdict === 'palm') {
      this.swallow(event);
    }
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    // A synthetic reset means "OrbitControls, forget this drag", never "the
    // finger left the glass". Honouring it would drop a live contact from the
    // group and let the next one — often the palm — be admitted alongside it.
    if (isSyntheticPointerEvent(event)) {
      return;
    }
    if (event.pointerType === 'pen') {
      this.penContact = false;
      this.lastPenActivityMs = this.now();
      return;
    }
    if (event.pointerType !== 'touch') {
      return;
    }
    const record = this.touchVerdicts.get(event.pointerId);
    this.touchVerdicts.delete(event.pointerId);
    // Swallow **only** for a palm verdict — never on an admit. The two-finger
    // controller dispatches a synthetic `pointercancel` for its admitted fingers
    // to reset OrbitControls; swallowing here would eat that (and real admitted
    // pointerups, breaking single-tap selection). Deleting an admit entry is
    // harmless: the finger has already passed the arbiter's gate.
    if (record?.verdict === 'palm') {
      this.swallow(event);
    }
  };

  private readonly onPointerOut = (event: PointerEvent): void => {
    if (isSyntheticPointerEvent(event)) {
      return;
    }
    // Pen left hover range (or the surface). Drop the "in contact" flag so the
    // grace window starts counting down; touch resumes once it elapses.
    if (event.pointerType === 'pen') {
      this.penContact = false;
      this.lastPenActivityMs = this.now();
    }
  };
}
