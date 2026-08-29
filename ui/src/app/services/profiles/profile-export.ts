import { Injectable, inject, signal } from '@angular/core';
import { FileExport } from '../file-export';
import { NotificationService } from '../notifications';
import { FilamentsStore } from './filaments-store';
import { LabelsStore } from './labels-store';
import { PrintProfilesStore } from './print-profiles-store';
import { PrintersStore } from './printers-store';
import {
  ProfilePersistence,
  type ProfileExportFormat,
  type ProfileLibrarySnapshot,
} from './profile-persistence';

/**
 * Downloads the user's profile library as TOML.
 *
 * The **engine** renders every export, in all three runtimes — there is no
 * TypeScript TOML writer here. That is deliberate: the exported files are the
 * same documents the engine reads back (`profiles.toml`), so a profile the user
 * exports today keeps working, and a setting added tomorrow appears in the
 * export without anyone touching this code.
 *
 * Where the data comes from differs by runtime, and only because the truth
 * does: native and cloud export the library persisted next to the engine (what
 * the CLI on that machine would read), while the web runtime — where the
 * browser is the engine — exports the library held in this tab.
 */
@Injectable({ providedIn: 'root' })
export class ProfileExport {
  private readonly persistence = inject(ProfilePersistence);
  private readonly fileExport = inject(FileExport);
  private readonly notifications = inject(NotificationService);
  private readonly printers = inject(PrintersStore);
  private readonly filaments = inject(FilamentsStore);
  private readonly processes = inject(PrintProfilesStore);
  private readonly labels = inject(LabelsStore);

  /** True while an export is being rendered, so the button can show progress. */
  readonly isExporting = signal(false);

  /** Render the library in the requested shape and hand it to the user. */
  async export(format: ProfileExportFormat): Promise<void> {
    if (this.isExporting()) {
      return;
    }
    this.isExporting.set(true);
    try {
      const artifact = await this.persistence.exportLibrary(format, this.snapshot());
      await this.fileExport.saveBytes(artifact.bytes, artifact.filename, {
        mime: artifact.mime,
        filters: [filterFor(format)],
        savedLabel: 'Profiles',
      });
    } catch (error) {
      this.notifications.error(
        'Export failed',
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      this.isExporting.set(false);
    }
  }

  /** The library as this tab currently holds it. */
  private snapshot(): ProfileLibrarySnapshot {
    return {
      printers: this.printers.items(),
      filaments: this.filaments.items(),
      processes: this.processes.items(),
      labels: this.labels.items(),
    };
  }
}

function filterFor(format: ProfileExportFormat): { name: string; extensions: string[] } {
  return format === 'bundle'
    ? { name: 'ZIP archive', extensions: ['zip'] }
    : { name: 'TOML', extensions: ['toml'] };
}
