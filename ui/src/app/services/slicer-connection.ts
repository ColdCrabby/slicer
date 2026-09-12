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

/**
 * How often to ping the engine while idle.
 *
 * A WebSocket whose peer has vanished without a close frame — an engine killed
 * outright, a proxy dropping an idle tunnel, a laptop resumed from sleep — stays
 * `OPEN` on this side indefinitely. Without traffic there is nothing to notice
 * it, so the badge reads "Connected" over a socket to nowhere and the next slice
 * waits on a reply that can never come. A ping turns that silence into an error
 * the retry ladder can act on.
 */
const HEARTBEAT_INTERVAL_MS = 15000;

/**
 * Grace period for a pong. The engine answers a `Ping` with a `Pong`; missing
 * two intervals in a row is treated as a dead socket.
 */
const HEARTBEAT_TIMEOUT_MS = 10000;

@Injectable({ providedIn: 'root' })
export class SlicerConnection {
  readonly #destroyRef = inject(DestroyRef);
  readonly #reconnect$ = new Subject<void>();

  #subject: WebSocketSubject<ServerMessage> | null = null;
  #retryCount = 0;
  #heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  #pongTimer: ReturnType<typeof setTimeout> | null = null;

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
      this.#stopHeartbeat();
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

    // Deliberately not limited to `failed`/`disconnected`: a socket wedged in
    // `connecting` is the state a user is most likely to reach for this in, and
    // refusing there made the button look broken.
    if (this.status() === 'connected') {
      return;
    }
    this.#stopHeartbeat();

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
          this.#startHeartbeat();
        },
      },
      closeObserver: {
        next: () => {
          this.#stopHeartbeat();
          if (this.status() === 'connected') {
            this.status.set('disconnected');
            this.lastError.set('Connection closed by server');
            // A close is not an error, so `catchError` never sees it and
            // nothing below schedules a retry. Left alone the app sat
            // permanently disconnected after any ordinary server restart,
            // recovering only if the tab happened to be hidden and re-shown.
            this.#scheduleRetry();
          }
        },
      },
    });

    return this.#subject.pipe(
      // Any message at all proves the socket is alive; a pong is just the one
      // we can provoke on demand.
      tap(() => this.#clearPongTimer()),
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

  /**
   * Begin probing the connection. Called on every successful open; the previous
   * timer (if any) is discarded first so reconnects never stack probes.
   */
  #startHeartbeat(): void {
    this.#stopHeartbeat();
    this.#heartbeatTimer = setInterval(() => {
      if (!this.isConnected()) {
        return;
      }
      this.send({ type: 'Ping' });
      // Only arm the deadline if one is not already running — a pong clears it,
      // so a pending timer means the previous probe is still unanswered.
      if (this.#pongTimer === null) {
        this.#pongTimer = setTimeout(() => {
          this.#pongTimer = null;
          this.lastError.set('The slicer engine stopped responding.');
          this.status.set('disconnected');
          this.#stopHeartbeat();
          // Tear the dead socket down so the reconnect builds a fresh one
          // rather than re-using a half-open handle.
          this.#subject?.complete();
          this.#subject = null;
          this.#scheduleRetry();
        }, HEARTBEAT_TIMEOUT_MS);
      }
    }, HEARTBEAT_INTERVAL_MS);
  }

  #stopHeartbeat(): void {
    if (this.#heartbeatTimer !== null) {
      clearInterval(this.#heartbeatTimer);
      this.#heartbeatTimer = null;
    }
    this.#clearPongTimer();
  }

  #clearPongTimer(): void {
    if (this.#pongTimer !== null) {
      clearTimeout(this.#pongTimer);
      this.#pongTimer = null;
    }
  }

  /**
   * Queue the next reconnect attempt, honouring the same backoff and attempt
   * ceiling as the error path so a flapping engine cannot be hammered.
   */
  #scheduleRetry(): void {
    this.#retryCount++;
    this.retryCount.set(this.#retryCount);

    if (this.#retryCount >= MAX_RETRIES) {
      this.status.set('failed');
      return;
    }

    const delayMs = Math.min(
      RETRY_DELAY_MS * Math.pow(2, this.#retryCount - 1),
      MAX_RETRY_DELAY_MS,
    );
    this.status.set('connecting');
    setTimeout(() => this.#reconnect$.next(), delayMs);
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
