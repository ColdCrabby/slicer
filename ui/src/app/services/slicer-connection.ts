import { DestroyRef, Injectable, computed, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { EMPTY, Observable, Subject, timer } from 'rxjs';
import { catchError, share, switchMap, tap } from 'rxjs/operators';
import { WebSocketSubject, webSocket } from 'rxjs/webSocket';
import { environment } from '../../environments/environment';
import { ClientMessage } from '../../generated/slicer-engine-ws-client-message-v1';
import { ServerMessage } from '../../generated/slicer-engine-ws-server-message-v1';

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected' | 'failed';

/**
 * Maximum reconnection attempts before permanently failing.
 * After this limit, only explicit `retry()` calls will re-attempt connection.
 */
const MAX_RETRIES = 3;

/**
 * Initial delay (ms) before first retry. Doubles on each subsequent attempt
 * (exponential backoff) to prevent overwhelming the server during outages.
 */
const RETRY_DELAY_MS = 2000;

/**
 * Maximum delay cap (ms) for exponential backoff to prevent unreasonably long waits.
 */
const MAX_RETRY_DELAY_MS = 30000;

@Injectable({ providedIn: 'root' })
export class SlicerConnection {
  readonly #destroyRef = inject(DestroyRef);
  readonly #reconnect$ = new Subject<void>();

  #subject: WebSocketSubject<ServerMessage> | null = null;
  #retryCount = 0;

  readonly status = signal<ConnectionStatus>('connecting');
  readonly retryCount = signal(0);
  readonly isFailed = computed(() => this.status() === 'failed');
  readonly isConnected = computed(() => this.status() === 'connected');
  readonly lastError = signal<string | null>(null);

  readonly messages$: Observable<ServerMessage>;
  readonly #cloudTransportEnabled: boolean;

  constructor() {
    this.#cloudTransportEnabled = this.isCloudTransportEnabled();
    if (!this.#cloudTransportEnabled) {
      this.status.set('disconnected');
      this.messages$ = EMPTY;
      return;
    }

    const shared$ = this.#reconnect$.pipe(
      switchMap(() => this.#connect()),
      share(),
      takeUntilDestroyed(this.#destroyRef),
    );

    this.messages$ = shared$;
    shared$.subscribe();

    this.#reconnect$.next();

    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible' && !this.isConnected()) {
        this.retry();
      }
    };

    document.addEventListener('visibilitychange', onVisibilityChange);
    this.#destroyRef.onDestroy(() => {
      document.removeEventListener('visibilitychange', onVisibilityChange);
    });
  }

  send(msg: ClientMessage): void {
    if (!this.isConnected()) {
      console.warn('[SlicerConnection] Cannot send message: not connected', msg);
      this.lastError.set('WebSocket not connected');
      return;
    }
    try {
      // Cast to ServerMessage is a protocol necessity — we're sending in ClientMessage
      // format but WebSocketSubject expects typed sends as-is; this is safe.
      this.#subject?.next(msg as unknown as ServerMessage);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Unknown send error';
      console.error('[SlicerConnection] Send failed:', errorMsg);
      this.lastError.set(errorMsg);
    }
  }

  retry(): void {
    if (!this.#cloudTransportEnabled) {
      return;
    }

    const current = this.status();
    if (current !== 'failed' && current !== 'disconnected') {
      return;
    }

    this.#retryCount = 0;
    this.retryCount.set(0);
    this.status.set('connecting');
    this.#reconnect$.next();
  }

  #connect(): Observable<ServerMessage> {
    this.#subject?.complete();

    this.#subject = webSocket<ServerMessage>({
      url: environment.wsUrl,
      openObserver: {
        next: () => {
          this.#retryCount = 0;
          this.retryCount.set(0);
          this.lastError.set(null);
          this.status.set('connected');
        },
      },
      closeObserver: {
        next: () => {
          if (this.status() === 'connected') {
            this.status.set('disconnected');
            this.lastError.set('Connection closed by server');
          }
        },
      },
    });

    return this.#subject.pipe(
      catchError((err: unknown) => {
        this.#retryCount++;
        this.retryCount.set(this.#retryCount);
        const errorMsg =
          err instanceof Error ? err.message : `WebSocket error (attempt ${this.#retryCount})`;
        this.lastError.set(errorMsg);

        if (this.#retryCount >= MAX_RETRIES) {
          this.status.set('failed');
          console.error(
            `[SlicerConnection] Failed after ${MAX_RETRIES} attempts. Use retry() to reconnect.`,
          );
          return EMPTY;
        }

        // Exponential backoff: delay = RETRY_DELAY_MS * 2^(attempt-1), capped at MAX_RETRY_DELAY_MS
        const delayMs = Math.min(
          RETRY_DELAY_MS * Math.pow(2, this.#retryCount - 1),
          MAX_RETRY_DELAY_MS,
        );
        this.status.set('connecting');
        console.info(
          `[SlicerConnection] Retrying in ${delayMs}ms (attempt ${this.#retryCount}/${MAX_RETRIES})`,
        );

        return timer(delayMs).pipe(
          tap(() => this.#reconnect$.next()),
          switchMap(() => EMPTY),
        );
      }),
    );
  }

  private isCloudTransportEnabled(): boolean {
    const globals = globalThis as unknown as {
      __TAURI__?: unknown;
      __TAURI_INTERNALS__?: unknown;
      navigator?: { userAgent?: string };
    };
    const isTauri =
      Boolean(globals.__TAURI__ || globals.__TAURI_INTERNALS__) ||
      Boolean(globals.navigator?.userAgent?.includes('Tauri'));
    if (isTauri) {
      return false;
    }

    return environment.runtimeMode === 'cloud';
  }
}
