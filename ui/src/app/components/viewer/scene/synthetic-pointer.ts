/**
 * Marker for pointer events the viewport synthesises for itself.
 *
 * When the two-finger controller takes over a gesture it dispatches a
 * `pointercancel` for each live finger so OrbitControls forgets its
 * half-started drag. That event is a lie told to one listener: the fingers are
 * still very much on the glass. Every *other* listener must ignore it, or it
 * corrupts state that tracks physical contacts — the palm-rejection arbiter
 * would forget which touches are down (losing the group verdict that keeps a
 * palm from joining a live pinch), and the touch tuning would drop
 * `zoomToCursor` mid-pinch.
 *
 * A symbol keyed off the event object is the cheapest reliable channel: it
 * survives dispatch (the same object instance is delivered), needs no wrapper
 * type, and cannot collide with anything the DOM defines.
 */
const SYNTHETIC_POINTER_EVENT = Symbol.for('slicer.viewer.syntheticPointerEvent');

/**
 * Tag an event the viewport is about to dispatch at itself, so state that
 * models *physical* contacts can skip it. See {@link isSyntheticPointerEvent}.
 */
export function markSyntheticPointerEvent<T extends Event>(event: T): T {
  (event as unknown as Record<symbol, boolean>)[SYNTHETIC_POINTER_EVENT] = true;
  return event;
}

/**
 * True for an event the viewport dispatched at itself rather than one the OS
 * reported. Such an event describes intent, never a real finger lifting.
 */
export function isSyntheticPointerEvent(event: Event): boolean {
  return (
    (event as unknown as Record<symbol, boolean | undefined>)[SYNTHETIC_POINTER_EVENT] === true
  );
}
