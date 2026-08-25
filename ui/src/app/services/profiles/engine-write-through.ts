import { type DestroyRef, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { EMPTY, Subject, defer, from, merge } from 'rxjs';
import { catchError, debounceTime, switchMap, tap } from 'rxjs/operators';
import type { ProfileCategory, ProfilePersistence } from './profile-persistence';

/** Lifecycle of a debounced write-through to the engine store. */
export type SaveStatus = 'idle' | 'pending' | 'saving' | 'error';

/** How long to coalesce rapid edits (e.g. dragging a color) into one save. */
export const SAVE_DEBOUNCE_MS = 250;

/**
 * Debounced, last-writer-wins write-through of one profile category to the
 * engine store.
 *
 * A burst of edits (dragging a color, typing a name) collapses into a single
 * `PUT` fired {@link SAVE_DEBOUNCE_MS} after the edits settle, instead of one
 * request per keystroke. A pending write is flushed immediately when the tab is
 * hidden or unloaded so a fast navigation never loses the latest edit; the PUT
 * uses `keepalive` so it survives page teardown.
 *
 * Inert on the browser-local (wasm) backend — there is no engine to write to.
 */
export class EngineWriteThrough {
  private readonly _status = signal<SaveStatus>('idle');
  /** `pending` while debouncing, `saving` in flight, `error` on failure. */
  readonly status = this._status.asReadonly();

  private readonly _error = signal<string | null>(null);
  /** Last save failure message, cleared on the next successful save. */
  readonly error = this._error.asReadonly();

  private readonly debounced$ = new Subject<void>();
  private readonly flush$ = new Subject<void>();

  constructor(
    private readonly persistence: ProfilePersistence,
    private readonly category: ProfileCategory,
    /** Snapshot of the current items to persist, read at save time. */
    private readonly snapshot: () => unknown[],
    destroyRef: DestroyRef,
  ) {
    // One pipeline: a debounced edit stream and an immediate flush stream both
    // feed a `switchMap` so the newest write supersedes any in-flight one.
    merge(this.debounced$.pipe(debounceTime(SAVE_DEBOUNCE_MS)), this.flush$)
      .pipe(
        switchMap(() => this.save()),
        takeUntilDestroyed(destroyRef),
      )
      .subscribe();

    const flushIfHidden = () => {
      if (document.visibilityState === 'hidden') {
        this.flush();
      }
    };
    document.addEventListener('visibilitychange', flushIfHidden);
    window.addEventListener('pagehide', () => this.flush());
    destroyRef.onDestroy(() => {
      document.removeEventListener('visibilitychange', flushIfHidden);
    });
  }

  /** Record a change and schedule a debounced write. */
  queue(): void {
    if (!this.persistence.isEngineBacked) {
      return;
    }
    this._status.set('pending');
    this.debounced$.next();
  }

  /** Write any pending change now, bypassing the debounce. */
  flush(): void {
    if (this._status() === 'pending' || this._status() === 'error') {
      this.flush$.next();
    }
  }

  private save() {
    return defer(() => {
      this._status.set('saving');
      return from(this.persistence.saveCategory(this.category, this.snapshot())).pipe(
        tap(() => {
          this._status.set('idle');
          this._error.set(null);
        }),
        catchError((err: unknown) => {
          this._status.set('error');
          this._error.set(err instanceof Error ? err.message : String(err));
          return EMPTY;
        }),
      );
    });
  }
}
