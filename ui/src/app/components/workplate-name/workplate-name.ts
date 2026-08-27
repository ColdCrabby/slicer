import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router } from '@angular/router';
import { filter, map, startWith } from 'rxjs';
import { SlicerFile } from '../../services/slicer-file';
import { WorkplateNames } from '../../services/workplate-names';

/**
 * Inline-editable title for the currently open workplate, shown in the titlebar
 * next to the brand. Mirrors the printer/filament rename pattern: a plain text
 * field that reads as a title until hovered/focused, persisting to
 * {@link WorkplateNames} on change. The key is the `:requestUuid` route segment,
 * so the field appears the moment a plate is open and hides everywhere else.
 */
@Component({
  selector: 'nexus-workplate-name',
  imports: [],
  templateUrl: './workplate-name.html',
  styleUrl: './workplate-name.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[hidden]': '!requestUuid()',
  },
})
export class WorkplateName {
  private readonly router = inject(Router);
  private readonly slicerFile = inject(SlicerFile);
  private readonly store = inject(WorkplateNames);

  /** The `:requestUuid` of the open plate (the persistence key), or null. */
  readonly requestUuid = toSignal(
    this.router.events.pipe(
      filter((event) => event instanceof NavigationEnd),
      startWith(null),
      map(() => this.#uuidFromUrl(this.router.url)),
    ),
    { initialValue: this.#uuidFromUrl(this.router.url) },
  );

  /** The stored custom name, if the plate was renamed. */
  readonly savedName = computed(() => this.store.nameFor(this.requestUuid()) ?? '');

  /** Fallback shown as placeholder when the plate has no custom name yet. */
  readonly placeholder = computed(() =>
    this.store.displayNameFor(
      this.requestUuid(),
      this.slicerFile.sourceFilename() ?? this.slicerFile.selectedFile()?.name,
    ),
  );

  rename(event: Event): void {
    const uuid = this.requestUuid();
    if (!uuid) {
      return;
    }
    this.store.setName(uuid, (event.target as HTMLInputElement).value);
  }

  /** Extract the workplate UUID from a `/slice/:requestUuid` URL. */
  #uuidFromUrl(url: string): string | null {
    const match = url.split(/[?#]/)[0].match(/^\/slice\/([^/]+)$/);
    if (!match) {
      return null;
    }
    const segment = decodeURIComponent(match[1]);
    return segment === 'new' ? null : segment;
  }
}
