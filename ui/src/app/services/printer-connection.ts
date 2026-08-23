import { Injectable, computed, inject, signal } from '@angular/core';
import { environment } from '../../environments/environment';
import type { ClientMessage } from '../../generated/slicer-engine-ws-client-message-v1';
import type { ServerMessage } from '../../generated/slicer-engine-ws-server-message-v1';
import type { PrinterConnection, PrinterProfile } from '../models/printer.model';
import { NotificationService } from './notifications';
import { SlicerConnection } from './slicer-connection';

/**
 * Live reachability of a printer.
 *
 * - `local`       — no network connection configured; a purely offline profile.
 * - `unknown`     — never probed.
 * - `checking`    — a probe is in flight.
 * - `online`      — the printer answered a status query.
 * - `offline`     — the host did not answer (unreachable / powered off).
 * - `unsupported` — connection kind not implemented yet.
 * - `cors`        — the host is reachable from the browser but blocks the
 *                   request via CORS (only ever seen on the direct-`fetch`
 *                   path used by the wasm/web build).
 * - `error`       — the host answered with an error (e.g. bad API key).
 */
export type PrinterProbeState =
  | 'local'
  | 'unknown'
  | 'checking'
  | 'online'
  | 'offline'
  | 'unsupported'
  | 'cors'
  | 'error';

export interface PrinterLiveStatus {
  state: PrinterProbeState;
  /** Short badge label. */
  label: string;
  /** Firmware/host state (`ready`, `error`, …), when known. */
  firmwareState?: string;
  /** Job state (`printing`, `paused`, …), when known. */
  printState?: string;
  /** Print progress 0–1, when a job is active. */
  progress?: number;
  /** Longer human-readable detail (tooltip). */
  message?: string;
  /** Epoch ms of the last probe. */
  checkedAt?: number;
}

const LOCAL_STATUS: PrinterLiveStatus = { state: 'local', label: 'Local profile' };
const UNKNOWN_STATUS: PrinterLiveStatus = { state: 'unknown', label: 'Not checked' };

/** How often (ms) to re-probe connected printers. */
const POLL_INTERVAL_MS = 30_000;

/**
 * Tracks live printer connectivity and drives "send to printer".
 *
 * **Transport preference (matches the engine's SSOT):** when the cloud
 * WebSocket is available, probes and uploads are performed **server-side**
 * (`CheckPrinter` / `SendToPrinter`). This is preferred because the request
 * originates from the slicer process, not the browser, so it is **not subject
 * to CORS** — Moonraker ships no permissive CORS headers, so a direct browser
 * request fails for most users.
 *
 * When no WebSocket is available (the wasm/`web` build), the service falls back
 * to a direct browser `fetch`. That path is expected to hit CORS for many
 * hosts; rather than reporting a misleading "offline", it distinguishes a
 * genuinely unreachable host from a reachable-but-CORS-blocked one (via a
 * follow-up `no-cors` probe) and surfaces an actionable `cors` state.
 */
@Injectable({ providedIn: 'root' })
export class PrinterConnectionService {
  private readonly ws = inject(SlicerConnection);
  private readonly notifications = inject(NotificationService);

  private readonly statusMap = signal<Record<string, PrinterLiveStatus>>({});

  /** Read-only view of every known printer status keyed by profile id. */
  readonly statuses = computed(() => this.statusMap());

  /**
   * Mirrors the cloud WebSocket connectivity. Consumers can depend on this in
   * an effect to re-probe printers once the server link comes up.
   */
  readonly serverConnected = this.ws.isConnected;

  constructor() {
    // Correlate server replies back to the originating printer card.
    this.ws.messages$.subscribe((msg: ServerMessage) => this.onServerMessage(msg));
  }

  /** Live status for a printer id, defaulting to `unknown`. */
  statusFor(printerId: string): PrinterLiveStatus {
    return this.statusMap()[printerId] ?? UNKNOWN_STATUS;
  }

  /** Probe every printer that has a network connection configured. */
  checkAll(printers: readonly PrinterProfile[]): void {
    for (const printer of printers) {
      this.check(printer);
    }
  }

  /** Start (or refresh) a probe for a single printer. */
  check(printer: PrinterProfile): void {
    const connection = printer.connection;
    if (!connection || connection.kind === 'none') {
      this.setStatus(printer.id, LOCAL_STATUS);
      return;
    }

    this.setStatus(printer.id, { state: 'checking', label: 'Checking…' });

    // In cloud mode the server always performs the probe (no CORS). If the
    // WebSocket isn't up yet, stay in `checking` — a reconnect-driven re-probe
    // (see the home dashboard effect) will pick it up rather than falling back
    // to a CORS-prone browser request.
    if (environment.runtimeMode === 'cloud') {
      if (this.ws.isConnected()) {
        this.sendWs({ type: 'CheckPrinter', printer_id: printer.id, connection });
      }
      return;
    }

    // Non-cloud (wasm/web, native): probe directly from the browser.
    void this.probeFromBrowser(printer.id, connection);
  }

  /**
   * Send the G-code sliced for `requestUuid` to a printer, optionally starting
   * the print. Result is surfaced via a notification.
   */
  sendToPrinter(
    printer: PrinterProfile,
    requestUuid: string,
    options: { filename?: string; start?: boolean } = {},
  ): void {
    const connection = printer.connection;
    if (!connection || connection.kind === 'none') {
      this.notifications.error('No connection', `${printer.name} has no printer connection set up.`);
      return;
    }

    if (this.canUseServer()) {
      this.sendWs({
        type: 'SendToPrinter',
        request_uuid: requestUuid,
        printer_id: printer.id,
        connection,
        filename: options.filename,
        start: options.start ?? false,
      });
      this.notifications.info('Sending to printer', `Uploading to ${printer.name}…`);
      return;
    }

    // Browser fallback: this reads the sliced G-code from the server's download
    // endpoint and pushes it to the printer directly. Only viable when a
    // download URL exists and the printer permits CORS — otherwise it fails the
    // same way a browser probe does.
    this.notifications.error(
      'Direct send unavailable',
      'Sending to a printer from the browser is blocked by CORS. Use the desktop app or the local server to send prints.',
    );
  }

  // ── internals ─────────────────────────────────────────────────────────────

  private onServerMessage(msg: ServerMessage): void {
    if (msg.type === 'PrinterStatus') {
      this.setStatus(msg.printer_id, this.fromServerStatus(msg));
    } else if (msg.type === 'PrinterSendResult') {
      if (msg.ok) {
        this.notifications.success('Sent to printer', msg.message);
      } else {
        this.notifications.error('Send failed', msg.message);
      }
    }
  }

  private fromServerStatus(msg: Extract<ServerMessage, { type: 'PrinterStatus' }>): PrinterLiveStatus {
    if (!msg.online) {
      return {
        state: 'offline',
        label: 'Offline',
        message: msg.message ?? undefined,
        checkedAt: Date.now(),
      };
    }
    const printState = msg.print_state ?? undefined;
    const label = printState === 'printing' ? 'Printing' : 'Online';
    return {
      state: 'online',
      label,
      firmwareState: msg.state ?? undefined,
      printState,
      progress: msg.progress ?? undefined,
      message: msg.message ?? undefined,
      checkedAt: Date.now(),
    };
  }

  /** True when the cloud WebSocket is connected and usable for RPC. */
  private canUseServer(): boolean {
    return environment.runtimeMode === 'cloud' && this.ws.isConnected();
  }

  private sendWs(msg: ClientMessage): void {
    this.ws.send(msg);
  }

  private setStatus(printerId: string, status: PrinterLiveStatus): void {
    this.statusMap.update((map) => ({ ...map, [printerId]: status }));
  }

  /**
   * Direct-from-browser Moonraker probe used when no server WebSocket is
   * available. Distinguishes unreachable from CORS-blocked to give an honest,
   * actionable status.
   */
  private async probeFromBrowser(printerId: string, connection: PrinterConnection): Promise<void> {
    if (connection.kind !== 'moonraker') {
      this.setStatus(printerId, {
        state: 'unsupported',
        label: 'Not supported',
        message: `${connection.kind} connections can only be checked from the desktop app or server.`,
      });
      return;
    }

    const base = buildBaseUrl(connection);
    if (!base) {
      this.setStatus(printerId, { state: 'error', label: 'No host', message: 'No host configured.' });
      return;
    }

    const url = `${base}/printer/objects/query?webhooks&print_stats&display_status`;
    const headers: Record<string, string> = {};
    if (connection.api_key) {
      headers['X-Api-Key'] = connection.api_key;
    }

    try {
      const resp = await fetch(url, { headers, signal: AbortSignal.timeout(5000) });
      if (!resp.ok) {
        this.setStatus(printerId, {
          state: 'error',
          label: 'Error',
          message: `Printer responded with HTTP ${resp.status}.`,
          checkedAt: Date.now(),
        });
        return;
      }
      const body = (await resp.json()) as MoonrakerQueryResponse;
      const status = body?.result?.status ?? {};
      this.setStatus(printerId, {
        state: 'online',
        label: status.print_stats?.state === 'printing' ? 'Printing' : 'Online',
        firmwareState: status.webhooks?.state,
        printState: status.print_stats?.state,
        progress: status.display_status?.progress,
        checkedAt: Date.now(),
      });
    } catch {
      // A normal fetch failure is ambiguous (network vs. CORS). A follow-up
      // `no-cors` probe resolves the ambiguity: if it succeeds (opaque), the
      // host is reachable but blocks CORS; if it also throws, it's unreachable.
      await this.classifyBrowserFailure(printerId, url);
    }
  }

  private async classifyBrowserFailure(printerId: string, url: string): Promise<void> {
    try {
      await fetch(url, { mode: 'no-cors', signal: AbortSignal.timeout(5000) });
      // Reached the host but the response is opaque → CORS is blocking us.
      this.setStatus(printerId, {
        state: 'cors',
        label: 'Blocked (CORS)',
        message:
          'The printer is reachable but blocks browser requests (CORS). Use the desktop app or the local server to connect, or enable CORS in your Moonraker config.',
        checkedAt: Date.now(),
      });
    } catch {
      this.setStatus(printerId, {
        state: 'offline',
        label: 'Offline',
        message: 'Could not reach the printer. Check the host address and that it is powered on.',
        checkedAt: Date.now(),
      });
    }
  }

  /** Interval used by consumers that want periodic refresh. */
  static readonly POLL_INTERVAL_MS = POLL_INTERVAL_MS;
}

interface MoonrakerQueryResponse {
  result?: {
    status?: {
      webhooks?: { state?: string; state_message?: string };
      print_stats?: { state?: string };
      display_status?: { progress?: number };
    };
  };
}

/** Normalize a connection into a base URL (`http://host[:port]`), or `null`. */
function buildBaseUrl(connection: PrinterConnection): string | null {
  const host = connection.host?.trim();
  if (!host) {
    return null;
  }
  let url = /^https?:\/\//i.test(host) ? host : `http://${host}`;
  url = url.replace(/\/+$/, '');
  if (connection.port != null) {
    const authority = url.split('://')[1] ?? '';
    const hasPort = (authority.split('/')[0] ?? '').includes(':');
    if (!hasPort) {
      url = `${url}:${connection.port}`;
    }
  }
  return url;
}
