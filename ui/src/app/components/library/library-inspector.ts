import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
  untracked,
  viewChild,
  type TemplateRef,
} from '@angular/core';
import {
  Button,
  FloatingService,
  Icon,
  IconButton,
  TooltipDirective,
  type FloatingRef,
} from '@coldcrabby/ui';
import { WorkplateTabSearch } from '../workplate-tabs/workplate-tab-search';
import { ObjectLibrary, type LibraryEntry } from '../../services/library';
import { LibraryActions } from '../../services/library/library-actions';
import { removeLabel, revealLabel } from './library-browser';
import { formatAgo, formatBytes, formatDateTime, formatExtents } from './library-format';
import { LibraryPreview } from './library-preview';

/** How long the remove button waits for its second press. */
const CONFIRM_MS = 3_000;

/**
 * One library model, up close: a live view, what it is, where it is kept, and
 * what to do with it.
 *
 * The live view loads the model's file; until it has, the card's thumbnail
 * stands in, so selecting a model never shows an empty pane.
 */
@Component({
  selector: 'nexus-library-inspector',
  imports: [Button, Icon, IconButton, LibraryPreview, TooltipDirective, WorkplateTabSearch],
  templateUrl: './library-inspector.html',
  styleUrl: './library-inspector.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LibraryInspector {
  protected readonly library = inject(ObjectLibrary);
  protected readonly actions = inject(LibraryActions);
  readonly #floating = inject(FloatingService);

  readonly entry = input.required<LibraryEntry>();
  /** Back to the library overview. */
  readonly dismiss = output<void>();

  private readonly nameInput = viewChild.required<ElementRef<HTMLInputElement>>('name');
  private readonly pickerPanel = viewChild.required<TemplateRef<unknown>>('pickerPanel');
  #picker: FloatingRef | null = null;
  /** The button the picker hangs from; a component host, so kept as the element. */
  #pickerTrigger: HTMLElement | null = null;

  protected readonly previewFile = signal<File | null>(null);
  protected readonly confirmingRemove = signal(false);
  #confirmTimer: ReturnType<typeof setTimeout> | null = null;

  protected readonly missing = computed(
    () => !(this.entry().locations ?? []).some((l) => !l.missing),
  );
  protected readonly thumbnail = computed(
    () => this.library.thumbnails().get(this.entry().id) ?? null,
  );
  protected readonly extents = computed(() => formatExtents(this.entry()));
  protected readonly fileSize = computed(() => formatBytes(this.entry().size));
  protected readonly firstSeen = computed(() => formatDateTime(this.entry().added_at));
  /** "Never", or how often and how recently — "3 times, last 2 days ago". */
  protected readonly usage = computed(() => {
    const { use_count: count, last_used_at: at } = this.entry();
    if (!count || !at) {
      return 'Never';
    }
    const ago = formatAgo(at);
    return `${count === 1 ? 'Once' : `${count} times`}, ${ago === 'just now' ? ago : `last ${ago}`}`;
  });
  protected readonly lastUsedExact = computed(() => {
    const at = this.entry().last_used_at;
    return at ? formatDateTime(at) : null;
  });
  /** Other files — re-exports, renamed downloads — that turned out to be this model. */
  protected readonly alsoSeenAs = computed(() =>
    Math.max(0, (this.entry().hashes?.length ?? 1) - 1),
  );
  protected readonly removeLabel = computed(() => removeLabel(this.entry()));
  protected readonly revealLabel = revealLabel();

  constructor() {
    inject(DestroyRef).onDestroy(() => this.closePicker());

    // Load the model for the live view, dropping a stale load.
    effect((onCleanup) => {
      const entry = this.entry();
      const missing = this.missing();
      let current = true;
      onCleanup(() => (current = false));
      // Untracked: both read signals — the thumbnail map, the confirm state —
      // and a thumbnail arriving for some other card must not reload this one.
      untracked(() => {
        this.previewFile.set(null);
        this.#disarm();
        this.library.ensureThumbnail(entry);
      });
      if (missing) {
        return;
      }
      void this.library.open(entry).then((file) => {
        if (current) {
          this.previewFile.set(file);
        }
      });
    });
  }

  /** Put the cursor in the name field, all of it selected. */
  focusName(): void {
    const input = this.nameInput().nativeElement;
    input.focus();
    input.select();
  }

  protected async rename(event: Event): Promise<void> {
    const entry = this.entry();
    const input = event.target as HTMLInputElement;
    const name = input.value.trim();
    if (!name || name === entry.name) {
      input.value = entry.name;
      return;
    }
    await this.library.rename(entry, name);
  }

  protected revert(input: HTMLInputElement): void {
    input.value = this.entry().name;
    input.blur();
  }

  /** Which open workplate to add this model to. */
  /**
   * The same searchable list the tab strip's chevron opens, dropped below the
   * button: pick a workplate and the model goes onto it. A second press on the
   * button closes it.
   */
  protected chooseWorkplate(button: HTMLElement): void {
    if (this.#picker) {
      this.closePicker();
      return;
    }
    this.#pickerTrigger = button;
    this.#picker = this.#floating.openTemplate(
      this.pickerPanel(),
      {},
      {
        reference: button,
        interactive: true,
        originElement: button,
        options: { placement: 'bottom-start', offset: 4, padding: 8 },
        onOutsidePointer: () => this.closePicker(),
      },
    );
  }

  protected addTo(uuid: string): void {
    this.closePicker();
    void this.actions.addToWorkplate(this.entry(), uuid);
  }

  /** From the keyboard, focus goes back to the button that opened it. */
  protected dismissPicker(): void {
    const trigger = this.#pickerTrigger;
    this.closePicker();
    trigger?.focus();
  }

  protected closePicker(): void {
    this.#picker?.close();
    this.#picker = null;
  }

  /** The keyboard's Delete: the same two presses as the bin. */
  requestRemove(): void {
    void this.remove();
  }

  /** Inline two-step confirm: the first press arms, the second removes. */
  protected async remove(): Promise<void> {
    if (!this.confirmingRemove()) {
      this.confirmingRemove.set(true);
      this.#confirmTimer = setTimeout(() => this.confirmingRemove.set(false), CONFIRM_MS);
      return;
    }
    const entry = this.entry();
    this.#disarm();
    this.dismiss.emit();
    await this.library.remove(entry);
  }

  protected disarm(): void {
    this.#disarm();
  }

  #disarm(): void {
    if (this.#confirmTimer) {
      clearTimeout(this.#confirmTimer);
      this.#confirmTimer = null;
    }
    this.confirmingRemove.set(false);
  }
}
