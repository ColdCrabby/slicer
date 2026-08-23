import { AccordionGroup, AccordionPanel, AccordionTrigger } from '@angular/aria/accordion';
import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterRenderEffect,
  computed,
  inject,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import Fuse, { type IFuseOptions } from 'fuse.js';
import { Sidebar } from '../nexus/sidebar/sidebar';
import { BrowserStorage } from '../services/browser-storage';
import { KeyboardShortcuts } from '../services/keyboard-shortcuts/keyboard-shortcuts';
import { Icon } from '../shared/icon/icon';
import { UserInputModality } from '../shared/input-modality/input-modality';
import { FieldHost } from './field-host/field-host';
import { FieldDef, SchemaGroup } from './models/field-def';
import { parseSchema } from './models/schema-parser';

export interface FieldChangeEvent {
  key: string;
  value: unknown;
}

const ACCORDION_STORAGE_KEY = 'schema-form-accordion';

/**
 * Schema-driven form container.
 *
 * Accepts any JSON Schema object and a flat value map, then renders
 * every property as the appropriate widget component. Fields are
 * visually grouped by their `x-group` schema extension value and
 * presented as collapsible accordion panels.
 *
 * Accordion expansion state is persisted to localStorage via BrowserStorage
 * so panels reopen in the same state after page refresh. Multiple panels
 * can be open simultaneously. All panels start closed by default.
 *
 * The component emits `fieldChange` events rather than mutating the
 * value directly, keeping the data flow unidirectional.
 *
 * @example
 * ```html
 * <se-schema-form
 *   [schema]="mySchema"
 *   [value]="currentSettings()"
 *   (fieldChange)="onFieldChange($event)"
 * />
 * ```
 */
/** A `FieldDef` annotated with the name of its parent group and its Fuse relevance score. */
export interface FieldDefWithGroup extends FieldDef {
  groupName: string;
  /** Fuse.js match score: 0 = perfect match, 1 = worst match. */
  score: number;
}

type FieldDefIndexed = FieldDef & { groupName: string };

const FUSE_OPTIONS: IFuseOptions<FieldDefIndexed> = {
  keys: [
    { name: 'title', weight: 0.7 },
    { name: 'key', weight: 0.5 },
    { name: 'description', weight: 0.3 },
    { name: 'groupName', weight: 0.2 },
  ],
  threshold: 0.35,
  ignoreLocation: true,
  minMatchCharLength: 2,
  shouldSort: true,
  includeScore: true,
};

@Component({
  selector: 'se-schema-form',
  standalone: true,
  imports: [FormsModule, Icon, FieldHost, AccordionGroup, AccordionPanel, AccordionTrigger],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './schema-form.component.html',
  styleUrl: './schema-form.component.scss',
})
export class SchemaForm {
  private readonly storage = inject(BrowserStorage);
  private readonly inputModality = inject(UserInputModality);
  private readonly sidebar = inject(Sidebar, { optional: true });
  readonly keyboardShortcuts = inject(KeyboardShortcuts);

  /** Raw JSON Schema object. Changing this input re-parses the schema. */
  readonly schema = input.required<Record<string, unknown>>();

  /**
   * Current values keyed by field name. Pass a partial object — missing
   * keys fall back to the schema default when rendering.
   */
  readonly value = input<Record<string, unknown>>({});

  /**
   * Optional allow-list of `x-group` names to display, in the given order.
   * When set, only those accordion groups render (used to categorise settings
   * by contract). Search always spans every group regardless of this filter.
   */
  readonly visibleGroups = input<readonly string[] | null>(null);

  /**
   * Optional map of group name → icon name. When a group has an entry its
   * accordion header shows that icon, giving each collapsible section a visual
   * anchor. Groups without an entry simply render without an icon.
   */
  readonly groupIcons = input<Record<string, string>>({});

  /** Emitted whenever the user changes a single field. */
  readonly fieldChange = output<FieldChangeEvent>();

  private readonly searchInputRef = viewChild<ElementRef<HTMLInputElement>>('searchInput');
  private readonly searchBarRef = viewChild<ElementRef<HTMLElement>>('searchBar');
  private readonly hostEl = inject<ElementRef<HTMLElement>>(ElementRef);

  protected readonly searchQuery = signal('');

  constructor() {
    this.keyboardShortcuts.schemaFormRef = this;
    inject(DestroyRef).onDestroy(() => (this.keyboardShortcuts.schemaFormRef = null));

    // Keep --schema-form-search-h in sync with the sticky search bar's height so
    // the sticky group headers can pin directly beneath it regardless of its
    // rendered size (font/spacing token changes, wrapping, etc.).
    let obs: ResizeObserver | null = null;
    afterRenderEffect({
      read: (onCleanup) => {
        const el = this.searchBarRef()?.nativeElement;
        obs?.disconnect();
        obs = null;
        if (!el) return;

        obs = new ResizeObserver(() => {
          // offsetHeight includes padding + border (the search bar has top
          // padding); contentRect would under-measure and tuck headers behind.
          const h = el.offsetHeight;
          this.hostEl.nativeElement.style.setProperty(
            '--schema-form-search-h',
            `${Math.round(h)}px`,
          );
        });
        obs.observe(el);

        onCleanup(() => {
          obs?.disconnect();
          obs = null;
        });
      },
    });
  }

  focusSearch(): void {
    this.sidebar?.expand();
    setTimeout(() => this.searchInputRef()?.nativeElement.focus({ preventScroll: true }), 0);
  }

  /** Every group parsed from the schema, unaffected by the visible filter. */
  private readonly allGroups = computed<SchemaGroup[]>(() => parseSchema(this.schema()).groups);

  /** Groups actually rendered in the accordion, honouring `visibleGroups`. */
  protected readonly groups = computed<SchemaGroup[]>(() => {
    const all = this.allGroups();
    const visible = this.visibleGroups();
    if (!visible) {
      return all;
    }
    const order = new Map(visible.map((name, index) => [name, index]));
    return all
      .filter((group) => order.has(group.name))
      .sort((a, b) => order.get(a.name)! - order.get(b.name)!);
  });

  /** All fields flattened with their group name, used to build the Fuse index. */
  private readonly flatFields = computed<FieldDefIndexed[]>(() =>
    this.allGroups().flatMap((g) => g.fields.map((f) => ({ ...f, groupName: g.name }))),
  );

  /**
   * Ranked search results when the user has typed a query.
   * Returns an empty array when the query is blank.
   */
  protected readonly searchResults = computed<FieldDefWithGroup[]>(() => {
    const query = this.searchQuery().trim();
    if (!query) {
      return [];
    }
    const fuse = new Fuse(this.flatFields(), FUSE_OPTIONS);
    return fuse.search(query).map((r) => ({ ...r.item, score: r.score ?? 0 }));
  });

  /**
   * Map of group names to their expanded state signals.
   * Created lazily as groups are encountered in the template.
   */
  private readonly expandedSignalMap = new Map<string, ReturnType<typeof signal<boolean>>>();

  protected getExpandedSignal(groupName: string): ReturnType<typeof signal<boolean>> {
    if (!this.expandedSignalMap.has(groupName)) {
      const isExpanded = this.isGroupExpandedInStorage(groupName);
      const sig = signal(isExpanded);
      this.expandedSignalMap.set(groupName, sig);
    }
    return this.expandedSignalMap.get(groupName)!;
  }

  protected onExpandedChange(groupName: string, groupEl: HTMLElement): void {
    const sig = this.getExpandedSignal(groupName);
    if (sig()) {
      // Defer until the expand animation has started so the element has its
      // final height. block:'nearest' scrolls the minimum distance to reveal
      // the whole group; if the panel is taller than the viewport the browser
      // aligns the top (heading) to the viewport top instead.
      setTimeout(() => groupEl.scrollIntoView({ behavior: 'smooth', block: 'nearest' }), 0);
    }
    this.persistExpandedState();
  }

  protected onFieldChange(key: string, value: unknown): void {
    this.fieldChange.emit({ key, value });
  }

  /**
   * On touch devices the AccordionGroup listens to `pointerdown` which fires
   * before the finger is lifted, so the panel opens on touch-start rather
   * than on a tap.  Stopping propagation at the trigger button prevents the
   * event from reaching the group's listener; `onGroupTriggerClick` then
   * handles the tap via the normal `click` event instead.
   */
  protected onGroupTriggerPointerDown(event: PointerEvent): void {
    if (this.inputModality.modality() === 'touch') {
      event.stopPropagation();
    }
  }

  protected onGroupTriggerClick(sig: ReturnType<typeof signal<boolean>>): void {
    if (this.inputModality.modality() === 'touch') {
      sig.set(!sig());
    }
  }

  private isGroupExpandedInStorage(groupName: string): boolean {
    const stored = this.storage.getJson<string[]>(ACCORDION_STORAGE_KEY, 'local');
    return stored ? stored.includes(groupName) : false;
  }

  private persistExpandedState(): void {
    const expanded: string[] = [];
    for (const [groupName, sig] of this.expandedSignalMap.entries()) {
      if (sig()) {
        expanded.push(groupName);
      }
    }
    this.storage.writeJson(ACCORDION_STORAGE_KEY, expanded, 'local');
  }
}
