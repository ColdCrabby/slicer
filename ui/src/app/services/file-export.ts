import { Injectable, inject } from '@angular/core';
import { save } from '@tauri-apps/plugin-dialog';
import { writeFile } from '@tauri-apps/plugin-fs';
import { isTauriHost, isTauriMobile } from '../runtime/domain/runtime-mode.util';
import { NotificationService } from './notifications';

/** A "Save as" file-type filter, as the native dialog expects it. */
export interface FileTypeFilter {
  /** Label shown in the dialog's type dropdown. */
  name: string;
  /** Extensions without the dot, e.g. `['gcode', 'gco']`. */
  extensions: string[];
}

/** Grace period before an object URL is released (see {@link FileExport.saveBytes}). */
const REVOKE_DELAY_MS = 60_000;

/**
 * Hands a file to the user, using whatever "save a file" means on the platform
 * the app is running on.
 *
 * There are three genuinely different idioms and no portable one:
 *
 * - **iOS/iPadOS** — there is no Save-As panel at all. The export idiom is the
 *   share sheet (Save to Files, AirDrop, Mail), which performs the copy itself.
 *   Tauri's `save()` *does* exist on iOS but writes an empty placeholder outside
 *   the sandbox, so the follow-up write lands nowhere and the user gets a
 *   0-byte file. The bytes are staged in the app's cache directory first,
 *   because the sandbox is always writable and `UIActivityViewController` needs
 *   a real file on disk.
 * - **Desktop (Tauri)** — the webview does not reliably honour `<a download>`
 *   against a custom `asset://` URL, so use the native Save-As dialog plus a
 *   filesystem write.
 * - **Browser** — an anchor with `download` and an object URL.
 *
 * Every download in the app goes through here so those platform quirks are
 * solved once, not per feature.
 */
@Injectable({ providedIn: 'root' })
export class FileExport {
  private readonly notifications = inject(NotificationService);

  /** Save in-memory bytes, e.g. an export rendered by the engine. */
  async saveBytes(
    bytes: Uint8Array,
    filename: string,
    options: { mime?: string; filters?: FileTypeFilter[]; savedLabel?: string } = {},
  ): Promise<void> {
    const blob = new Blob([bytes as BlobPart], {
      type: options.mime ?? 'application/octet-stream',
    });
    const url = URL.createObjectURL(blob);
    try {
      await this.saveFromUrl(url, filename, options);
    } finally {
      // The browser path only *starts* the download on click; revoking in the
      // same task can cancel it. Release on a later task instead — the native
      // paths have already read the blob by the time they return.
      setTimeout(() => URL.revokeObjectURL(url), REVOKE_DELAY_MS);
    }
  }

  /** Save whatever a URL resolves to — a blob URL, `asset://`, or an HTTP URL. */
  async saveFromUrl(
    url: string,
    filename: string,
    options: { filters?: FileTypeFilter[]; savedLabel?: string } = {},
  ): Promise<void> {
    if (isTauriMobile()) {
      await this.shareViaSystemSheet(url, filename);
      return;
    }

    if (isTauriHost()) {
      try {
        const destination = await save({
          defaultPath: filename,
          ...(options.filters ? { filters: options.filters } : {}),
        });
        if (!destination) {
          return; // user cancelled the dialog
        }
        const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
        await writeFile(destination, bytes);
        // Always name the destination: on desktop the user picked a path and
        // needs to know where the file actually went.
        this.notifications.success(
          'Saved',
          options.savedLabel
            ? `${options.savedLabel} saved to ${destination}`
            : `Saved to ${destination}`,
        );
      } catch (error) {
        this.notifications.error('Save failed', messageOf(error));
      }
      return;
    }

    const link = document.createElement('a');
    link.href = url;
    link.download = filename;
    link.click();
  }

  /**
   * iOS export: stage the file inside the app's own cache directory, then hand
   * it to `UIActivityViewController`.
   */
  private async shareViaSystemSheet(url: string, filename: string): Promise<void> {
    try {
      const [{ invoke }, { appCacheDir, join }, { mkdir, writeFile: writeFsFile }] =
        await Promise.all([
          import('@tauri-apps/api/core'),
          import('@tauri-apps/api/path'),
          import('@tauri-apps/plugin-fs'),
        ]);

      const directory = await appCacheDir();
      await mkdir(directory, { recursive: true }).catch(() => undefined);
      const staged = await join(directory, filename);

      const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
      await writeFsFile(staged, bytes);

      // Anchor the iPad popover near the top-right, where the triggering
      // control lives. An unanchored share sheet raises and terminates the app.
      await invoke('share_file', {
        path: staged,
        x: Math.round(window.innerWidth * 0.9),
        y: Math.round(window.innerHeight * 0.1),
      });
    } catch (error) {
      this.notifications.error('Share failed', messageOf(error));
    }
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
