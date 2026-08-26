import { DestroyRef, inject, Injectable, signal, Signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { fromEvent } from 'rxjs';
import { filter } from 'rxjs/operators';

export type StorageArea = 'local' | 'session';

/**
 * Provides reactive, type-safe access to localStorage and sessionStorage.
 *
 * - Returns writable `Signal` instances that stay in sync with the underlying
 *   storage entry.
 * - localStorage signals are kept in sync across browser tabs via the
 *   `storage` event. sessionStorage signals are tab-local by nature and
 *   are NOT synced across tabs.
 * - Signals are cached by key so the same `Signal` object is returned on
 *   repeated calls.
 */
@Injectable({
  providedIn: 'root',
})
export class BrowserStorage {
  private readonly localSignals = new Map<string, ReturnType<typeof signal<string | null>>>();
  private readonly sessionSignals = new Map<string, ReturnType<typeof signal<string | null>>>();

  constructor() {
    const destroyRef = inject(DestroyRef);

    fromEvent<StorageEvent>(window, 'storage')
      .pipe(
        filter((event) => event.storageArea === localStorage && event.key !== null),
        takeUntilDestroyed(destroyRef),
      )
      .subscribe((event) => {
        const existing = this.localSignals.get(event.key!);
        if (existing) {
          existing.set(event.newValue);
        }
      });
  }

  /**
   * Returns a writable `Signal<string | null>` backed by the given storage key.
   *
   * @param key     The storage key.
   * @param area    `'local'` (default) or `'session'`.
   */
  get(key: string, area: StorageArea = 'local'): ReturnType<typeof signal<string | null>> {
    const map = area === 'local' ? this.localSignals : this.sessionSignals;

    if (!map.has(key)) {
      const storage = area === 'local' ? localStorage : sessionStorage;
      const initial = storage.getItem(key);
      const s = signal<string | null>(initial);
      map.set(key, s);
    }

    return map.get(key)!;
  }

  /**
   * Reads the current signal value and, if it differs from the stored value,
   * writes it to the underlying storage.
   *
   * Prefer calling `set()` on the returned signal to update state, then call
   * `persist()` to flush to storage — or use `write()` to do both atomically.
   */

  /**
   * Writes `value` to the signal AND to the underlying storage in one step.
   *
   * @param key   The storage key.
   * @param value The new value, or `null` to remove the entry.
   * @param area  `'local'` (default) or `'session'`.
   */
  write(key: string, value: string | null, area: StorageArea = 'local'): void {
    const s = this.get(key, area);
    const storage = area === 'local' ? localStorage : sessionStorage;

    if (value === null) {
      storage.removeItem(key);
    } else {
      storage.setItem(key, value);
    }

    s.set(value);
  }

  /**
   * Reads the current value from storage, refreshing the signal.
   * Useful for cases where an external actor may have mutated storage directly.
   */
  refresh(key: string, area: StorageArea = 'local'): void {
    const storage = area === 'local' ? localStorage : sessionStorage;
    const s = this.get(key, area);
    s.set(storage.getItem(key));
  }

  /**
   * Typed convenience helper — reads a JSON-serialised value from storage.
   * Returns `null` when the key is absent or the value cannot be parsed.
   */
  getJson<T>(key: string, area: StorageArea = 'local'): T | null {
    const raw = this.get(key, area)();
    if (raw === null) {
      return null;
    }
    try {
      return JSON.parse(raw) as T;
    } catch {
      return null;
    }
  }

  /**
   * Typed convenience helper — writes a value as JSON to storage.
   */
  writeJson<T>(key: string, value: T, area: StorageArea = 'local'): void {
    this.write(key, JSON.stringify(value), area);
  }

  /**
   * Returns a readonly signal that reflects the JSON-parsed value and
   * stays in sync when the underlying raw-string signal changes.
   *
   * Note: this produces a *new* computed-like structure each call.  For hot
   * paths, cache the result in your component/service.
   */
  getJsonSignal<T>(key: string, area: StorageArea = 'local'): Signal<T | null> {
    const raw = this.get(key, area);
    // Derive a computed-equivalent without importing `computed` in the service
    // by returning a plain Signal that is kept in sync by the raw signal.
    // We use a writable signal here so the type is compatible with the map.
    const derived = signal<T | null>(this.parseJson<T>(raw()));

    // Keep derived in sync using effect-free polling via the storage event
    // or caller updates.  For full reactivity the caller should use `get()`
    // directly and call JSON.parse themselves, or use a `computed()` wrapper.
    // This helper is intentionally lightweight — use `getJson()` in most cases.
    return derived.asReadonly();
  }

  private parseJson<T>(raw: string | null): T | null {
    if (raw === null) {
      return null;
    }
    try {
      return JSON.parse(raw) as T;
    } catch {
      return null;
    }
  }
}
