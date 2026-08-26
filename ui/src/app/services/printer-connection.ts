import { Injectable, computed, inject, signal } from '@angular/core';
import { environment } from '../../environments/environment';
import type { ClientMessage } from '../../generated/slicer-engine-ws-client-message-v1';
import type { ServerMessage } from '../../generated/slicer-engine-ws-server-message-v1';
import type {
  BedShape,
  PrinterConnection,
  PrinterConnectionKind,
  PrinterProfile,
} from '../models/printer.model';
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

/**
 * Everything a single-URL probe could learn about a printer, for the setup
 * wizard to prefill itself. Every hardware field is optional: detection is
 * best-effort. When `reachable` is false only `message` is meaningful.
 */
export interface PrinterDetectionResult {
  /** The host that was probed (echoed for correlation). */
  host: string;
  /** The host answered at least one probe. */
  reachable: boolean;
  /** Detected transport, or `none` when nothing answered. */
  kind: PrinterConnectionKind;
  /** Human-readable summary (a success note or the failure reason). */
  message?: string;
  /** Friendly name (Klipper hostname), when known. */
  name?: string;
  /** Model designation, when known. */
  model?: string;
  /** Manufacturer / firmware family, when known. */
  vendor?: string;
  /** G-code dialect the firmware speaks (`marlin`, `klipper`). */
  firmware?: string;
  /** Bed shape (rectangular / circular), when known. */
  bedShape?: BedShape;
  /** Bed width / diameter (mm), when known. */
  bedWidth?: number;
  /** Bed depth (mm), when known. */
  bedDepth?: number;
  /** Max Z height (mm), when known. */
  bedHeight?: number;
  /** True for delta / center-origin machines, when known. */
  originAtCenter?: boolean;
  /** Nozzle diameter (mm), when known. */
  nozzleDiameterMm?: number;
}

const LOCAL_STATUS: PrinterLiveStatus = { state: 'local', label: 'Local profile' };
const UNKNOWN_STATUS: PrinterLiveStatus = { state: 'unknown', label: 'Not checked' };

/** How often (ms) to re-probe connected printers. */
const POLL_INTERVAL_MS = 30_000;

/** How long (ms) to wait for a server-side detection reply before giving up. */
const DETECT_TIMEOUT_MS = 15_000;

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

  /** Pending server-side detections, keyed by the probed host. */
  private readonly pendingDetections = new Map<string, (result: PrinterDetectionResult) => void>();

  /** In-flight server sends → their progress task, keyed by `${printerId}:${uuid}`. */
  private readonly sends = new Map<
    string,
    { taskId: string; timer: ReturnType<typeof setInterval> }
  >();

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
   * Probe a single URL and report everything we can learn about the printer
   * (kind, bed volume, nozzle, kinematics) so the setup wizard can prefill
   * itself.
   *
   * Prefers the server-side probe (no CORS) when the cloud WebSocket is up,
   * exactly like {@link check}. Otherwise it falls back to a direct browser
   * probe, which is expected to hit CORS for many Moonraker hosts.
   */
  detectPrinter(host: string): Promise<PrinterDetectionResult> {
    const trimmed = host.trim();
    if (!trimmed) {
      return Promise.resolve({
        host,
        reachable: false,
        kind: 'none',
        message: 'Enter a printer address first.',
      });
    }
    if (this.canUseServer()) {
      return this.detectViaServer(trimmed);
    }
    return this.detectFromBrowser(trimmed);
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
      this.notifications.error(
        'No connection',
        `${printer.name} has no printer connection set up.`,
      );
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
      this.beginSendProgress(printer, requestUuid);
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
      this.finishSend(msg);
    } else if (msg.type === 'PrinterDetected') {
      const resolve = this.pendingDetections.get(msg.host);
      if (resolve) {
        this.pendingDetections.delete(msg.host);
        resolve(this.fromServerDetection(msg));
      }
    }
  }

  /**
   * Show upload progress in the docked scene strip. The backend streams no
   * byte-level progress (a single multipart POST, one terminal result), so the
   * bar eases toward 90 % to feel alive, then {@link finishSend} snaps it to
   * 100 % when the result lands.
   */
  private beginSendProgress(printer: PrinterProfile, requestUuid: string): void {
    const key = this.sendKey(printer.id, requestUuid);
    this.clearSend(key);

    const verb = 'Uploading';
    const taskId = this.notifications.progress('Sending to printer', `${verb} to ${printer.name}…`);
    const timer = setInterval(() => {
      const task = this.notifications.tasks().find((t) => t.id === taskId);
      if (!task) {
        clearInterval(timer);
        return;
      }
      const next = task.progress + (90 - task.progress) * 0.12;
      this.notifications.updateProgress(taskId, Math.min(90, Math.round(next)));
    }, 140);

    this.sends.set(key, { taskId, timer });
  }

  private finishSend(msg: Extract<ServerMessage, { type: 'PrinterSendResult' }>): void {
    const key = this.sendKey(msg.printer_id, msg.request_uuid);
    const entry = this.sends.get(key);
    if (entry) {
      clearInterval(entry.timer);
      this.sends.delete(key);
      if (msg.ok) {
        this.notifications.updateProgress(entry.taskId, 100);
        this.notifications.completeProgress(entry.taskId, 'Sent to printer', msg.message);
      } else {
        this.notifications.failProgress(entry.taskId, 'Send failed', msg.message);
      }
    } else if (msg.ok) {
      this.notifications.success('Sent to printer', msg.message);
    } else {
      this.notifications.error('Send failed', msg.message);
    }

    if (msg.ok) {
      this.notifications.celebrate(
        msg.started ? 'Print started' : 'Sent to printer',
        msg.message,
        msg.started ? 'printer' : 'cloud-upload',
      );
    }
  }

  private sendKey(printerId: string, requestUuid: string): string {
    return `${printerId}:${requestUuid}`;
  }

  private clearSend(key: string): void {
    const entry = this.sends.get(key);
    if (entry) {
      clearInterval(entry.timer);
      this.notifications.dismissTask(entry.taskId);
      this.sends.delete(key);
    }
  }

  private fromServerStatus(
    msg: Extract<ServerMessage, { type: 'PrinterStatus' }>,
  ): PrinterLiveStatus {
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

  /** Register a pending detection and ask the server to probe `host`. */
  private detectViaServer(host: string): Promise<PrinterDetectionResult> {
    return new Promise((resolve) => {
      // If two probes for the same host overlap, retire the earlier one.
      const previous = this.pendingDetections.get(host);
      if (previous) {
        previous({ host, reachable: false, kind: 'none', message: 'Superseded by a newer probe.' });
      }

      const timeout = setTimeout(() => {
        if (this.pendingDetections.delete(host)) {
          resolve({
            host,
            reachable: false,
            kind: 'none',
            message: 'The slicer did not answer in time. Check that it is running and try again.',
          });
        }
      }, DETECT_TIMEOUT_MS);

      this.pendingDetections.set(host, (result) => {
        clearTimeout(timeout);
        resolve(result);
      });

      this.sendWs({ type: 'DetectPrinter', host });
    });
  }

  private fromServerDetection(
    msg: Extract<ServerMessage, { type: 'PrinterDetected' }>,
  ): PrinterDetectionResult {
    return {
      host: msg.host,
      reachable: msg.reachable,
      kind: msg.kind,
      message: msg.message ?? undefined,
      name: msg.name ?? undefined,
      model: msg.model ?? undefined,
      vendor: msg.vendor ?? undefined,
      firmware: msg.firmware ?? undefined,
      bedShape: msg.bed_shape ?? undefined,
      bedWidth: msg.bed_width ?? undefined,
      bedDepth: msg.bed_depth ?? undefined,
      bedHeight: msg.bed_height ?? undefined,
      originAtCenter: msg.origin_at_center ?? undefined,
      nozzleDiameterMm: msg.nozzle_diameter_mm ?? undefined,
    };
  }

  /**
   * Direct-from-browser detection used when no server WebSocket is available.
   * Mirrors the engine's probe order: Moonraker first (richest metadata), then
   * the OctoPrint / PrusaLink `/api/version` banner. Expected to hit CORS for
   * many Moonraker hosts in the web build.
   */
  private async detectFromBrowser(host: string): Promise<PrinterDetectionResult> {
    const base = buildBaseUrl({ host } as PrinterConnection);
    if (!base) {
      return { host, reachable: false, kind: 'none', message: 'Enter a valid printer address.' };
    }

    const moonraker = await this.detectMoonrakerFromBrowser(host, base);
    if (moonraker) {
      return moonraker;
    }

    const apiVersion = await this.detectApiVersionFromBrowser(host, base);
    if (apiVersion) {
      return apiVersion;
    }

    return {
      host,
      reachable: false,
      kind: 'none',
      message:
        'Could not identify a printer at that address. It may be off, or it blocks browser requests (CORS) — try the desktop app or local server.',
    };
  }

  private async detectMoonrakerFromBrowser(
    host: string,
    base: string,
  ): Promise<PrinterDetectionResult | null> {
    try {
      const info = await fetch(`${base}/printer/info`, { signal: AbortSignal.timeout(5000) });
      if (!info.ok) {
        return null;
      }
      const result = ((await info.json()) as MoonrakerInfoResponse)?.result ?? {};
      if (result.state == null && result.hostname == null) {
        return null;
      }

      const detection: PrinterDetectionResult = {
        host,
        reachable: true,
        kind: 'moonraker',
        vendor: 'Klipper',
        firmware: 'klipper',
        name: result.hostname || undefined,
      };

      try {
        const query = await fetch(`${base}/printer/objects/query?configfile&toolhead`, {
          signal: AbortSignal.timeout(5000),
        });
        if (query.ok) {
          const status = ((await query.json()) as MoonrakerObjectsResponse)?.result?.status ?? {};
          enrichFromMoonrakerObjects(detection, status);
        }
      } catch {
        // Enrichment is best-effort; a bare Moonraker id is still useful.
      }

      detection.message = detection.name
        ? `Found Klipper printer \u201c${detection.name}\u201d.`
        : 'Found a Klipper (Moonraker) printer.';
      return detection;
    } catch {
      return null;
    }
  }

  private async detectApiVersionFromBrowser(
    host: string,
    base: string,
  ): Promise<PrinterDetectionResult | null> {
    try {
      const resp = await fetch(`${base}/api/version`, { signal: AbortSignal.timeout(5000) });
      if (!resp.ok) {
        return null;
      }
      const body = (await resp.json()) as ApiVersionResponse;
      const banner = [body?.text, body?.server]
        .filter((s): s is string => typeof s === 'string')
        .join(' ')
        .toLowerCase();

      if (banner.includes('prusa')) {
        return {
          host,
          reachable: true,
          kind: 'prusalink',
          vendor: 'Prusa',
          firmware: 'marlin',
          message: 'Found a PrusaLink printer.',
          name: body?.hostname || undefined,
        };
      }
      if (banner.includes('octoprint')) {
        return {
          host,
          reachable: true,
          kind: 'octoprint',
          message: 'Found an OctoPrint host. Add its API key to finish setup.',
        };
      }
      return null;
    } catch {
      return null;
    }
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
      this.setStatus(printerId, {
        state: 'error',
        label: 'No host',
        message: 'No host configured.',
      });
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

interface MoonrakerInfoResponse {
  result?: { state?: string; hostname?: string };
}

interface MoonrakerObjectsStatus {
  configfile?: {
    settings?: {
      printer?: { kinematics?: string };
      extruder?: { nozzle_diameter?: number };
    };
  };
  toolhead?: { axis_maximum?: number[]; axis_minimum?: number[] };
}

interface MoonrakerObjectsResponse {
  result?: { status?: MoonrakerObjectsStatus };
}

interface ApiVersionResponse {
  text?: string;
  server?: string;
  hostname?: string;
}

/**
 * Pull bed dimensions, kinematics, and nozzle diameter out of a Moonraker
 * `printer/objects/query?configfile&toolhead` status payload. Mirrors the
 * engine's `enrich_from_moonraker_objects`.
 */
function enrichFromMoonrakerObjects(
  detection: PrinterDetectionResult,
  status: MoonrakerObjectsStatus,
): void {
  const settings = status.configfile?.settings;
  const isDelta = (settings?.printer?.kinematics ?? '').toLowerCase() === 'delta';
  detection.bedShape = isDelta ? 'circular' : 'rectangular';
  detection.originAtCenter = isDelta;

  const max = status.toolhead?.axis_maximum ?? [];
  const min = status.toolhead?.axis_minimum ?? [];
  const span = (i: number): number | undefined => {
    const hi = Number(max[i]);
    if (!Number.isFinite(hi)) {
      return undefined;
    }
    const lo = Number(min[i]);
    const value = Number.isFinite(lo) && lo < 0 ? hi - lo : hi;
    return value > 0 ? Math.round(value * 10) / 10 : undefined;
  };

  const width = span(0);
  if (width != null) {
    detection.bedWidth = width;
  }
  const depth = span(1);
  if (depth != null) {
    detection.bedDepth = depth;
  }
  const height = Number(max[2]);
  if (Number.isFinite(height)) {
    detection.bedHeight = Math.round(height * 10) / 10;
  }

  const nozzle = Number(settings?.extruder?.nozzle_diameter);
  if (Number.isFinite(nozzle) && nozzle > 0) {
    detection.nozzleDiameterMm = nozzle;
  }
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
