const STORAGE_KEY = 'slicer.client-id';

let cached: string | null = null;

/**
 * An opaque id for *this* view of the slicer.
 *
 * It exists for exactly one comparison: when the engine announces that a
 * workplate changed, a client that made the change itself must not be offered a
 * refresh away from what its user just did. Everything else about it is
 * deliberately absent — it says nothing about who anyone is, it is never sent
 * anywhere but this slicer's own engine, and a client that omits it is simply
 * told about every change, which is the safe direction to be wrong in.
 *
 * Scoped to the browser tab rather than the browser: two tabs of the same
 * slicer are two independent views of a plate and **should** tell each other
 * when one of them moves something. A reload keeps the id, and keeping it is
 * harmless — a reloaded tab refetches the plate anyway.
 */
export function clientId(): string {
  if (cached) {
    return cached;
  }
  try {
    const stored = sessionStorage.getItem(STORAGE_KEY);
    if (stored) {
      cached = stored;
      return stored;
    }
  } catch {
    // Private windows and blocked site data both land here. A per-load id is
    // still a correct id; it only means a reload is treated as a new view.
  }

  const minted =
    typeof globalThis.crypto?.randomUUID === 'function'
      ? globalThis.crypto.randomUUID()
      : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  cached = minted;
  try {
    sessionStorage.setItem(STORAGE_KEY, minted);
  } catch {
    // Memory-only for this load.
  }
  return minted;
}

/** Header the engine reads the id back from. */
export const CLIENT_ID_HEADER = 'X-Client-Id';
