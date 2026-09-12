import { Injectable, computed, inject, signal } from '@angular/core';
import { environment } from '../../environments/environment';
import type { ClientMessage } from '../../generated/slicer-engine-ws-client-message-v1';
import type {
  DetectionFinding,
  DetectionOption,
  DetectionQuestion,
  ServerMessage,
} from '../../generated/slicer-engine-ws-server-message-v1';
import { isTauriHost } from '../runtime/domain/runtime-mode.util';
import type {
  BedShape,
  PrinterConnection,
  PrinterConnectionKind,
  PrinterProfile,
} from '../models/printer.model';
import { NotificationService } from './notifications';
import { SlicerConnection } from './slicer-connection';

export type { DetectionFinding, DetectionOption, DetectionQuestion };

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
  'local' | 'unknown' | 'checking' | 'online' | 'offline' | 'unsupported' | 'cors' | 'error';

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
  /**
   * Sparse `SlicingParams` overlay read straight off the machine's own config.
   * Merged into the printer profile's `params` bag like a preset's overrides.
   */
  params?: Record<string, unknown>;
  /** Where each applied value came from, so the wizard can show its work. */
  findings?: DetectionFinding[];
  /**
   * Setup decisions the config could not make for us. Each carries a suggested
   * answer the wizard applies up front, so they refine a finished profile
   * rather than blocking one.
   */
  questions?: DetectionQuestion[];
}

const LOCAL_STATUS: PrinterLiveStatus = { state: 'local', label: 'Local profile' };
const UNKNOWN_STATUS: PrinterLiveStatus = { state: 'unknown', label: 'Not checked' };

/** How often (ms) to re-probe connected printers. */
const POLL_INTERVAL_MS = 30_000;

/** How long (ms) to wait for a server-side detection reply before giving up. */
const DETECT_TIMEOUT_MS = 70_000;

/** Browser-side timeout for the lightweight Moonraker info/toolhead probes. */
const DETECT_FAST_REQUEST_TIMEOUT_MS = 20_000;

/** Browser-side timeout for heavy Moonraker config payloads. */
const DETECT_SLOW_REQUEST_TIMEOUT_MS = 25_000;
const BROWSER_PROBE_TIMEOUT_MS = 5_000;

/**
 * Tracks live printer connectivity and drives "send to printer".
 *
 * **Transport preference (matches the engine's SSOT):** always talk to the
 * printer over the OS network line, never the browser, whenever one is
 * available. When the cloud WebSocket is up, probes/uploads run **server-side**
 * (`CheckPrinter` / `SendToPrinter`); in the desktop app they run **in the
 * native process** via Tauri commands (`printer_check` / `printer_detect` /
 * `printer_send`). Both are preferred because the request originates from the
 * slicer, not the browser, so it is **not subject to CORS** — Moonraker ships
 * no permissive CORS headers, so a direct browser request fails for most users.
 *
 * Only the pure wasm/`web` build has no OS network line; there the service falls
 * back to a direct browser `fetch`. That path is expected to hit CORS for many
 * hosts; rather than reporting a misleading "offline", it distinguishes a
 * genuinely unreachable host from a reachable-but-CORS-blocked one (via a
 * follow-up `no-cors` probe) and surfaces an actionable `cors` state.
 *
 * On Chromium builds with Local Network Access enabled, browser probes also send
 * `targetAddressSpace: 'local'` to trigger/grant local-network permission where
 * needed. This can unblock policy-level local-network checks, but it does not
 * bypass printer-side CORS headers.
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

    // Prefer the server-side probe (no CORS), exactly like {@link
    // detectPrinter} and {@link sendToPrinter}. `canUseServer()` (not the raw
    // `cloud` environment constant) correctly detects Tauri at runtime: the
    // desktop build also ships the `cloud` environment but its WebSocket never
    // connects, so checking the constant alone used to leave native `check()`
    // silently doing nothing instead of using the native transport.
    if (this.canUseServer()) {
      this.sendWs({ type: 'CheckPrinter', printer_id: printer.id, connection });
      return;
    }
    // Native desktop (Tauri): probe from the engine process over the OS network
    // stack (no browser CORS), mirroring the cloud WebSocket probe.
    if (isTauriHost()) {
      void this.probeViaNative(printer.id, connection);
      return;
    }
    if (environment.runtimeMode === 'cloud') {
      // WebSocket isn't up yet — stay in `checking`; a reconnect-driven
      // re-probe (see the home dashboard effect) will pick it up rather than
      // falling back to a CORS-prone browser request.
      return;
    }

    // Pure web (wasm) build: no OS network line available, so probe directly
    // from the browser and honestly surface CORS when it blocks us.
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
    if (isTauriHost()) {
      return this.detectViaNative(trimmed);
    }
    if (environment.runtimeMode === 'web') {
      return this.detectFromBrowser(trimmed);
    }
    // Cloud build whose WebSocket isn't connected yet — don't fall back to a
    // CORS-prone browser probe that would misreport a reachable printer.
    return Promise.resolve({
      host: trimmed,
      reachable: false,
      kind: 'none',
      message: 'Not connected to the slicer yet — try again in a moment.',
    });
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

    // Native desktop (Tauri): upload from the engine process over the OS network
    // stack (no browser CORS), mirroring the cloud WebSocket upload.
    if (isTauriHost()) {
      this.beginSendProgress(printer, requestUuid);
      void this.sendViaNative(printer, requestUuid, connection, options);
      return;
    }

    // Browser fallback (pure web build): pushing to the printer directly is
    // blocked the same way a browser probe is (CORS). Use the desktop app or the
    // local/cloud server instead.
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
        resolve(this.fromServerDetection(msg.host, msg));
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
            message:
              'Detection timed out. Some Klipper setups can take up to about a minute; try again, or verify the host address.',
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

  /**
   * Map the engine's wire shape onto the camelCase result the wizard consumes.
   *
   * The single funnel for all three transports — server WebSocket, native
   * command and wasm — so a new detection field is wired up once.
   */
  private fromServerDetection(host: string, msg: DetectedPayload): PrinterDetectionResult {
    return {
      host,
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
      params: (msg.params as Record<string, unknown> | undefined) ?? undefined,
      findings: msg.findings ?? undefined,
      questions: msg.questions ?? undefined,
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

  /**
   * Probe Moonraker from the browser, then hand the raw replies to the
   * engine's own derivation through wasm.
   *
   * The `fetch` calls have to happen here — this path exists precisely because
   * there is no server to make them — but the *interpretation* does not. In
   * the web build the browser is the engine, so it reads a printer's config
   * with the same rules the server and the desktop app use rather than a
   * second copy of them in TypeScript.
   *
   * Stages mirror `src/printer/transport.rs`: cheap and load-bearing first,
   * the large `configfile` payload last, each one optional after `/printer/info`.
   */
  private async detectMoonrakerFromBrowser(
    host: string,
    base: string,
  ): Promise<PrinterDetectionResult | null> {
    const info = await this.fetchJson(`${base}/printer/info`, DETECT_FAST_REQUEST_TIMEOUT_MS);
    if (!info) {
      return null;
    }

    // Sequential, not parallel: a printer is a single-board computer on the
    // end of a LAN, and four concurrent requests is how you make the slow one
    // time out.
    const fast = DETECT_FAST_REQUEST_TIMEOUT_MS;
    const toolhead = await this.fetchJson(`${base}/printer/objects/query?toolhead`, fast);
    const objectList = await this.fetchJson(`${base}/printer/objects/list`, fast);
    const bedMesh = await this.fetchJson(`${base}/printer/objects/query?bed_mesh`, fast);
    const configfile = await this.fetchJson(
      `${base}/printer/objects/query?configfile`,
      DETECT_SLOW_REQUEST_TIMEOUT_MS,
    );

    const derive = await loadKlipperDerivation();
    if (!derive) {
      return {
        host,
        reachable: true,
        kind: 'moonraker',
        firmware: 'klipper',
        message: 'Found a Klipper printer, but this build cannot read its settings.',
      };
    }

    const detection = derive({ info, toolhead, objectList, bedMesh, configfile });
    if (!detection) {
      return null;
    }
    return this.fromServerDetection(host, detection);
  }

  /**
   * GET a printer endpoint and parse its JSON, or `undefined` on any failure.
   * Every stage after the identifying one is best-effort: a refusal, a CORS
   * rejection or a timeout costs that stage's findings and nothing more.
   */
  private async fetchJson(url: string, timeoutMs: number): Promise<unknown | undefined> {
    try {
      const resp = await fetch(url, { ...this.localNetworkRequestInit(timeoutMs) });
      return resp.ok ? await resp.json() : undefined;
    } catch {
      return undefined;
    }
  }

  private async detectApiVersionFromBrowser(
    host: string,
    base: string,
  ): Promise<PrinterDetectionResult | null> {
    try {
      const resp = await fetch(`${base}/api/version`, {
        ...this.localNetworkRequestInit(BROWSER_PROBE_TIMEOUT_MS),
      });
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
    return environment.runtimeMode === 'cloud' && !isTauriHost() && this.ws.isConnected();
  }

  // ── native (Tauri) transport ──────────────────────────────────────────────
  //
  // The desktop app probes/uploads from the engine process over the OS network
  // stack (`slicer_engine::printer`, via `reqwest`) — the same code path as the
  // cloud WebSocket — so it is **not** subject to browser CORS. Each command
  // returns the same field shape as the corresponding WS server message (minus
  // its envelope), so the existing `fromServer*` mappers are reused verbatim.

  /** Invoke a Tauri command; resolves `null` if the bridge is unavailable. */
  private async invokeNative<T>(command: string, args: Record<string, unknown>): Promise<T | null> {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      return await invoke<T>(command, args);
    } catch {
      return null;
    }
  }

  /** Native equivalent of {@link probeFromBrowser} — no CORS. */
  private async probeViaNative(printerId: string, connection: PrinterConnection): Promise<void> {
    const report = await this.invokeNative<NativeStatusReport>('printer_check', { connection });
    if (!report) {
      this.setStatus(printerId, {
        state: 'error',
        label: 'Error',
        message: 'Could not reach the desktop runtime to check the printer.',
        checkedAt: Date.now(),
      });
      return;
    }
    this.setStatus(
      printerId,
      this.fromServerStatus({ type: 'PrinterStatus', printer_id: printerId, ...report }),
    );
  }

  /** Native equivalent of {@link detectFromBrowser} — no CORS. */
  private async detectViaNative(host: string): Promise<PrinterDetectionResult> {
    const detection = await this.invokeNative<DetectedPayload>('printer_detect', { host });
    if (!detection) {
      return {
        host,
        reachable: false,
        kind: 'none',
        message: 'Could not reach the desktop runtime to probe the printer.',
      };
    }
    return this.fromServerDetection(host, detection);
  }

  /** Native equivalent of the server-side upload — no CORS. */
  private async sendViaNative(
    printer: PrinterProfile,
    requestUuid: string,
    connection: PrinterConnection,
    options: { filename?: string; start?: boolean },
  ): Promise<void> {
    const result = await this.invokeNative<NativeSendResult>('printer_send', {
      connection,
      filename: options.filename ?? null,
      start: options.start ?? false,
    });
    this.finishSend({
      type: 'PrinterSendResult',
      printer_id: printer.id,
      request_uuid: requestUuid,
      ok: result?.ok ?? false,
      message: result?.message ?? 'Could not reach the desktop runtime to send the print.',
      started: result?.started ?? false,
    });
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
      const resp = await fetch(url, {
        ...this.localNetworkRequestInit(BROWSER_PROBE_TIMEOUT_MS),
        headers,
      });
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
    const permissionState = await this.localNetworkPermissionState();
    try {
      await fetch(url, {
        ...this.localNetworkRequestInit(BROWSER_PROBE_TIMEOUT_MS),
        mode: 'no-cors',
      });
      // Reached the host but the response is opaque → CORS is blocking us.
      this.setStatus(printerId, {
        state: 'cors',
        label: 'Blocked (CORS)',
        message:
          'The printer is reachable, but its API blocks browser reads (CORS). If prompted, allow local-network access in Chrome, then use the desktop app/local server or enable CORS in Moonraker.',
        checkedAt: Date.now(),
      });
    } catch {
      if (permissionState === 'denied') {
        this.setStatus(printerId, {
          state: 'error',
          label: 'Permission denied',
          message:
            'Chrome blocked local-network access for this site. Allow local-network access in site settings, then retry.',
          checkedAt: Date.now(),
        });
        return;
      }
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

  private localNetworkRequestInit(timeoutMs: number): LocalNetworkRequestInit {
    return { signal: AbortSignal.timeout(timeoutMs), targetAddressSpace: 'local' };
  }

  private async localNetworkPermissionState(): Promise<PermissionState | null> {
    if (!globalThis.isSecureContext) {
      return null;
    }

    const permissions = (navigator as NavigatorWithLocalNetworkPermissions).permissions;
    if (!permissions?.query) {
      return null;
    }

    try {
      const status = await permissions.query({ name: 'local-network-access' });
      return status.state;
    } catch {
      return null;
    }
  }
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

/** JSON from the native `printer_check` command (WS `PrinterStatus` minus envelope). */
interface NativeStatusReport {
  online: boolean;
  state?: string | null;
  print_state?: string | null;
  progress?: number | null;
  message?: string | null;
}

/**
 * The engine's `PrinterDetection`, in its own snake_case wire shape.
 *
 * Identical whether it arrives from the native `printer_detect` command, the
 * `PrinterDetected` WebSocket message or the wasm derivation — which is the
 * point: one derivation, three transports, one shape to map.
 * {@link PrinterConnectionService.fromServerDetection} is that single mapping.
 */
interface DetectedPayload {
  reachable: boolean;
  kind: PrinterConnectionKind;
  message?: string | null;
  name?: string | null;
  model?: string | null;
  vendor?: string | null;
  firmware?: string | null;
  bed_shape?: BedShape | null;
  bed_width?: number | null;
  bed_depth?: number | null;
  bed_height?: number | null;
  origin_at_center?: boolean | null;
  nozzle_diameter_mm?: number | null;
  params?: Record<string, unknown> | null;
  findings?: DetectionFinding[] | null;
  questions?: DetectionQuestion[] | null;
}

/** JSON from the native `printer_send` command (WS `PrinterSendResult` minus envelope). */
interface NativeSendResult {
  ok: boolean;
  message: string;
  started: boolean;
}

interface ApiVersionResponse {
  text?: string;
  server?: string;
  hostname?: string;
}

interface LocalNetworkRequestInit extends RequestInit {
  targetAddressSpace?: 'local';
}

type NavigatorWithLocalNetworkPermissions = Navigator & {
  permissions?: Permissions & {
    query(
      permissionDesc: PermissionDescriptor | { name: 'local-network-access' },
    ): Promise<PermissionStatus>;
  };
};

/** The raw Moonraker replies the engine's derivation takes, all optional but `info`. */
interface KlipperProbes {
  info: unknown;
  toolhead?: unknown;
  objectList?: unknown;
  bedMesh?: unknown;
  configfile?: unknown;
}

type KlipperDerivation = (probes: KlipperProbes) => DetectedPayload | null;

/** Resolved once; `null` in a build whose wasm bundle omits the binding. */
let klipperDerivation: KlipperDerivation | null | undefined;

/**
 * Load the engine's Klipper derivation out of the wasm bundle.
 *
 * Only the `web-slicer` bundle carries it — the lean viewer-only build has no
 * profile system to detect *into* — so the export is treated as optional and
 * its absence surfaces as an honest message rather than a crash.
 */
async function loadKlipperDerivation(): Promise<KlipperDerivation | null> {
  if (klipperDerivation !== undefined) {
    return klipperDerivation;
  }
  try {
    const wasm = (await import('../../generated/scene-wasm/scene_engine')) as unknown as {
      default: (options: { module_or_path: string }) => Promise<unknown>;
      deriveKlipperDetection?: KlipperDerivation;
    };
    await wasm.default({ module_or_path: 'scene_engine_bg.wasm' });
    klipperDerivation = wasm.deriveKlipperDetection ?? null;
  } catch {
    klipperDerivation = null;
  }
  return klipperDerivation;
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
