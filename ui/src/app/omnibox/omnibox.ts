import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  Injector,
  computed,
  effect,
  inject,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { Router } from '@angular/router';
import { Icon } from '@coldcrabby/ui';
import { MarkdownComponent } from 'ngx-markdown';
import { FieldHost } from '../schema-form/field-host/field-host';
import { focusConfigureTarget } from '../pages/settings/configure-scroll';
import { Slicer } from '../services/slicer';
import { WorkplateNames } from '../services/workplate-names';
import { WorkplateSettingsStore } from '../services/workplate-settings';
import {
  browseScope,
  describeValue,
  searchOmniboxEntries,
  type OmniboxEntry,
} from './omnibox-entries';
import { OmniboxService } from './omnibox-service';
import { collectOmniboxEntries, omniboxScopeDefs } from './scope-registry';
import {
  lockSuggestedScope,
  parseScopeQuery,
  popLockedScope,
  suggestedScope,
  type ScopeDef,
  type ScopeParse,
} from './scope-tokens';

/** One keyboard row: a scope to lock, or a result to act on. */
interface OmniboxRow {
  readonly key: string;
  readonly scope: ScopeDef | null;
  readonly entry: OmniboxEntry | null;
}

/**
 * The omnibox palette: one search over the whole app, with scope chips to keep
 * it honest.
 *
 * The palette owns interaction only — what the scopes are, what their tokens
 * are and what they serve all live in the registry, so this component never
 * learns a scope's name except through a `ScopeDef`. Opened through
 * {@link OmniboxService} (see the shell's `@defer` mount): nothing here runs
 * until the user asks for it.
 */
@Component({
  selector: 'nexus-omnibox',
  imports: [Icon, FieldHost, MarkdownComponent],
  templateUrl: './omnibox.html',
  styleUrl: './omnibox.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class OmniboxPalette {
  private readonly state = inject(OmniboxService);
  private readonly injector = inject(Injector);
  private readonly router = inject(Router);
  private readonly slicer = inject(Slicer);
  private readonly workplateSettings = inject(WorkplateSettingsStore);
  private readonly names = inject(WorkplateNames);

  /** The registry's scopes, in suggestion order — read once, never mutated. */
  private readonly defs = omniboxScopeDefs();

  protected readonly query = signal('');
  protected readonly locked = signal<readonly string[]>([]);
  protected readonly entries = signal<readonly OmniboxEntry[]>([]);
  protected readonly loading = signal(true);
  protected readonly activeIndex = signal(0);
  /** The quick-setting being edited inline, if any. */
  protected readonly editing = signal<OmniboxEntry | null>(null);
  protected readonly applied = signal(false);

  protected readonly parse = computed(() => parseScopeQuery(this.query(), this.defs));
  protected readonly rest = computed(() => this.parse().rest.trim());
  protected readonly suggestion = computed(() => suggestedScope(this.parse(), this.locked()));

  /**
   * What the list shows: matches for a query, a locked scope's wares for an
   * empty query behind a lock, or the scope cards on a bare open.
   */
  protected readonly rows = computed<readonly OmniboxRow[]>(() => {
    if (this.rest()) {
      return searchOmniboxEntries(this.entries(), this.rest(), this.locked()).map((entry) => ({
        key: entry.id,
        scope: null,
        entry,
      }));
    }
    if (this.locked().length) {
      return browseScope(this.entries(), this.locked()).map((entry) => ({
        key: entry.id,
        scope: null,
        entry,
      }));
    }
    return this.defs.map((def) => ({ key: `scope:${def.id}`, scope: def, entry: null }));
  });

  protected readonly activeRow = computed(() => this.rows()[this.activeIndex()] ?? null);

  /** The live composed values of the open plate, keyed as the engine spells them. */
  protected readonly currentValues = computed(
    () => this.slicer.settings() as unknown as Readonly<Record<string, unknown>>,
  );

  /** The open plate's override diff — which settings were already changed here. */
  protected readonly plateOverrides = computed<Readonly<Record<string, unknown>>>(
    () => this.workplateSettings.settingsFor(this.slicer.currentRequestUuid()).overrides ?? {},
  );

  /**
   * Where the edited value comes from: an override on this plate, or the
   * profile default. Flips live as the edit lands — the badge and the truth
   * are the same signal read.
   */
  protected readonly originLabel = computed(() => {
    const entry = this.editing();
    return entry && this.isPlateOverride(entry) ? 'This plate' : 'Profile default';
  });

  protected readonly plateName = computed(() => {
    const uuid = this.slicer.currentRequestUuid();
    return (uuid && this.names.displayNameFor(uuid, null)) || 'this plate';
  });

  private readonly input = viewChild<ElementRef<HTMLInputElement>>('omniboxInput');
  private readonly host = inject(ElementRef<HTMLElement>);

  constructor() {
    // Fresh start every open — the last search is a stranger's context.
    effect(() => {
      if (!this.state.open()) {
        return;
      }
      const prefill = this.state.takePrefill();
      this.editing.set(null);
      this.applied.set(false);
      this.locked.set([]);
      this.query.set(prefill ?? '');
      this.activeIndex.set(0);
      this.loading.set(true);
      void collectOmniboxEntries(this.injector).then((all) => {
        if (!this.state.open()) {
          return;
        }
        this.entries.set(all);
        this.loading.set(false);
      });
      // The palette mounts through `@defer`; the input exists a tick later.
      setTimeout(() => this.input()?.nativeElement.focus({ preventScroll: true }));
    });

    // Escape has to work even when the palette holds no focus at all — a click
    // on its chrome, a focus stolen by a popover, a screen-reader shortcut —
    // and the input's and panel's own handlers never see those keys. This
    // listener sits on the document in the *capture* phase so it also runs
    // ahead of the global shortcut table (whose Escape presses would otherwise
    // act on the app behind the palette in the same stroke), and steps aside
    // whenever focus is inside the palette, where Escape is staged — editor
    // first, then close — by the handlers that own it.
    effect((onCleanup) => {
      if (!this.state.open()) {
        return;
      }
      const onKey = (event: KeyboardEvent): void => {
        if (event.key !== 'Escape') {
          return;
        }
        const focus = document.activeElement;
        if (focus instanceof Node && this.host.nativeElement.contains(focus)) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        this.state.hide();
      };
      document.addEventListener('keydown', onKey, { capture: true });
      onCleanup(() => document.removeEventListener('keydown', onKey, { capture: true }));
    });

    // A thumbnail is produced for the row the user is actually pointed at —
    // hovered or walked to — never for the whole result list at once.
    effect(() => {
      const row = this.activeRow();
      untracked(() => row?.entry?.ensureThumbnail?.());
    });

    // Keep the highlighted row honest as the list shrinks and grows.
    effect(() => {
      const count = this.rows().length;
      if (this.activeIndex() >= count) {
        this.activeIndex.set(Math.max(0, count - 1));
      }
    });

    // Follow the highlight, without scrolling the page behind the palette.
    effect(() => {
      const index = this.activeIndex();
      untracked(() => {
        if (!this.state.open() || this.editing()) {
          return;
        }
        document.getElementById(`omnibox-row-${index}`)?.scrollIntoView({ block: 'nearest' });
      });
    });
  }

  protected labelFor(id: string): string {
    return this.defs.find((def) => def.id === id)?.label ?? id;
  }

  protected iconFor(id: string): string {
    return this.defs.find((def) => def.id === id)?.icon ?? 'search';
  }

  protected describe(entry: OmniboxEntry): string {
    return describeValue(entry.field, this.valueOf(entry));
  }

  protected isPlateOverride(entry: OmniboxEntry): boolean {
    return !!entry.fieldKey && entry.fieldKey in this.plateOverrides();
  }

  protected valueOf(entry: OmniboxEntry): unknown {
    return entry.fieldKey ? this.currentValues()[entry.fieldKey] : undefined;
  }

  protected onQuery(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
    this.activeIndex.set(0);
  }

  protected onKey(event: KeyboardEvent): void {
    switch (event.key) {
      case 'Escape': {
        event.preventDefault();
        event.stopPropagation();
        if (this.editing()) {
          this.stopEditing();
        } else {
          this.state.hide();
        }
        return;
      }
      case 'Tab': {
        // Tab accepts the suggested scope — and only then, so tabbing through
        // the palette's own focusables still works the browser's way.
        const parse = this.parse();
        if (suggestedScope(parse, this.locked())) {
          event.preventDefault();
          event.stopPropagation();
          this.lockTo(parse);
        }
        return;
      }
      case 'Backspace': {
        // An empty query with chips: the last Backspace peels a scope, like a
        // shell prompt's history unwind.
        if (!this.query() && this.locked().length) {
          event.preventDefault();
          event.stopPropagation();
          this.locked.set(popLockedScope(this.locked()));
          this.activeIndex.set(0);
        }
        return;
      }
      case 'ArrowDown':
      case 'ArrowUp': {
        const count = this.rows().length;
        if (!count) {
          return;
        }
        event.preventDefault();
        event.stopPropagation();
        const delta = event.key === 'ArrowDown' ? 1 : -1;
        this.activeIndex.set((this.activeIndex() + delta + count) % count);
        return;
      }
      case 'Enter': {
        const row = this.activeRow();
        if (row) {
          event.preventDefault();
          event.stopPropagation();
          this.activate(row);
        }
        return;
      }
    }
  }

  /** Escape from inside the editor's controls, where the input never sees it. */
  protected onPanelKey(event: KeyboardEvent): void {
    if (event.key !== 'Escape') {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (this.editing()) {
      this.stopEditing();
    } else {
      this.state.hide();
    }
  }

  protected lockTo(parse: ScopeParse): void {
    const next = lockSuggestedScope(parse, this.locked());
    if (next === this.locked()) {
      return;
    }
    this.locked.set(next);
    this.query.set(parse.rest);
    this.activeIndex.set(0);
    this.input()?.nativeElement.focus();
  }

  protected lockScope(id: string): void {
    if (this.locked().includes(id)) {
      return;
    }
    this.locked.set([...this.locked(), id]);
    this.query.set('');
    this.activeIndex.set(0);
    this.input()?.nativeElement.focus();
  }

  protected removeScope(id: string): void {
    this.locked.set(this.locked().filter((scope) => scope !== id));
    this.activeIndex.set(0);
    this.input()?.nativeElement.focus();
  }

  protected hover(index: number): void {
    this.activeIndex.set(index);
  }

  protected activate(row: OmniboxRow): void {
    if (row.scope) {
      this.lockScope(row.scope.id);
      return;
    }
    const entry = row.entry!;
    switch (entry.kind) {
      case 'quickset':
        this.applied.set(false);
        this.editing.set(entry);
        return;
      case 'command':
        void entry.run?.();
        this.state.hide();
        return;
      case 'navigate':
        void this.router
          .navigate([entry.path!], { queryParams: entry.queryParams ?? {} })
          .then(() => {
            if (entry.target) {
              focusConfigureTarget(entry.target);
            }
          });
        this.state.hide();
        return;
    }
  }

  /**
   * A click on the darkened surroundings puts the palette away.
   *
   * On `click`, not `pointerdown`: the pointer path removes the overlay before
   * the browser's follow-up click, which then lands on whatever app control
   * sits beneath the cursor — a dismissal that also pressed the button behind
   * it. Click fires after the gesture is over, so the same press cannot mean
   * both.
   */
  protected onBackdrop(event: MouseEvent): void {
    if (event.target === event.currentTarget) {
      this.state.hide();
    }
  }

  protected stopEditing(): void {
    this.editing.set(null);
    this.applied.set(false);
    this.input()?.nativeElement.focus();
  }

  protected applyQuickset(entry: OmniboxEntry, value: unknown): void {
    entry.apply?.(value);
    this.applied.set(true);
  }

  protected openInEditor(link: { path: string; queryParams?: Record<string, string> }): void {
    void this.router.navigate([link.path], { queryParams: link.queryParams ?? {} });
    this.state.hide();
  }
}
