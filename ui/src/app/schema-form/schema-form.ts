import {
  AccordionContent,
  AccordionGroup,
  AccordionPanel,
  AccordionTrigger,
} from '@angular/aria/accordion';
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
import { Viewport } from '../services/viewport';
import { Icon, UserInputModality } from '@coldcrabby/ui';
import { FieldHost } from './field-host/field-host';
import { SettingsOutline, type OutlineFieldJump } from './outline/settings-outline';
import { buildOutline, filterOutline, type OutlineSection } from './models/outline';
import { noticeForField } from './field-exceptions/field-exceptions';
import { controlFor } from './models/field-control';
import { FieldDef, SchemaGroup } from './models/field-def';
import { parseSchema } from './models/schema-parser';
import {
  type Tier,
  deepestTier,
  deeperOf,
  filterRelevantGroups,
  isFieldInTier,
  isTierAtMost,
  orderGroupsByTier,
  nextTier,
  shallowestTier,
  tierOf,
} from './models/relevance';
import { SettingsDetailPreference } from '../services/settings-detail-preference';

export interface FieldChangeEvent {
  key: string;
  value: unknown;
}

const ACCORDION_STORAGE_KEY = 'schema-form-accordion';

/**
 * How far each group has been revealed, persisted per group.
 *
 * A stated preference outranks the default the same way the accordion's own
 * expand state does: someone who works in Advanced all day should not re-open
 * it every session. This is not a mode switch — it is per section, it never
 * changes the shape of the app, and the panel still opens quiet for anyone who
 * has not asked.
 */
const TIER_STORAGE_KEY = 'schema-form-revealed-tiers';

/**
 * How far the *panel* is revealed — which whole sections are listed at all.
 *
 * Separate from the per-group key above: that one answers "how deep inside this
 * section", this one answers "which sections exist for me right now". Some
 * groups hold nothing but advanced or expert settings (Quality, Thumbnail, Time
 * estimate), and listing their headers in the everyday view offers the reader a
 * section that opens onto nothing.
 */
const PANEL_TIER_STORAGE_KEY = 'schema-form-revealed-panel-tier';

/** Depth of each tier, for comparing "is there anything deeper here?". */
const TIER_RANK: Record<Tier, number> = { everyday: 0, advanced: 1, expert: 2 };

/** Marker class on the row an outline jump landed on, while the hint lasts. */
const JUMP_CLASS = 'is-jump-target';

/** How long that hint lasts. Long enough to find, short enough not to nag. */
const JUMP_FLASH_MS = 1400;

/**
 * How many times a jump re-looks for its target, and how long it waits between
 * tries. A section's fields are built lazily when it first expands, so the
 * element a jump is aiming at does not exist in the frame the outline closes in.
 */
const JUMP_RETRIES = 4;
const JUMP_RETRY_MS = 60;

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
  imports: [
    FormsModule,
    Icon,
    FieldHost,
    AccordionGroup,
    AccordionPanel,
    AccordionTrigger,
    AccordionContent,
    SettingsOutline,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './schema-form.component.html',
  styleUrl: './schema-form.component.scss',
})
export class SchemaForm {
  private readonly storage = inject(BrowserStorage);
  private readonly settingsDetail = inject(SettingsDetailPreference);
  private readonly inputModality = inject(UserInputModality);
  private readonly sidebar = inject(Sidebar, { optional: true });
  private readonly viewport = inject(Viewport);
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

  /**
   * Keys whose value deviates from whatever the caller considers the baseline —
   * for the slice panel, the resolved preset stack. Marked fields get a quiet
   * accent rule and their group a dot, so a value that will follow its profile
   * is distinguishable at a glance from one that has been pinned. Empty by
   * default, which renders no marks at all.
   */
  readonly modifiedKeys = input<ReadonlySet<string>>(new Set());

  /** Emitted whenever the user changes a single field. */
  readonly fieldChange = output<FieldChangeEvent>();

  private readonly searchInputRef = viewChild<ElementRef<HTMLInputElement>>('searchInput');
  private readonly searchBarRef = viewChild<ElementRef<HTMLElement>>('searchBar');
  private readonly hostEl = inject<ElementRef<HTMLElement>>(ElementRef);

  protected readonly searchQuery = signal('');

  /**
   * Placeholder for the search box.
   *
   * The keyboard hint is only useful where there is a keyboard: on a phone
   * "(⌘+f)" is a shortcut the user cannot press, and it is long enough to push
   * the words that matter out of a narrow field.
   */
  protected readonly searchPlaceholder = computed(() => {
    if (this.outlineOpen()) {
      return 'Filter outline…';
    }
    return this.viewport.isHandheld()
      ? 'Search settings…'
      : `Search settings… (${this.keyboardShortcuts.shortcutFor('focus-settings-search')})`;
  });

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

  /**
   * Every group parsed from the schema, unaffected by the visible filter.
   *
   * Array parameters are dropped here. A fan curve and a set of pause triggers
   * are structured lists with dedicated editors elsewhere; there is no generic
   * control that can edit one, and offering the fallback widget rendered a
   * single input for a list of objects. Asking `controlFor` rather than testing
   * the type keeps that judgement in the one place the profile editors read it
   * from too.
   */
  private readonly allGroups = computed<SchemaGroup[]>(() =>
    parseSchema(this.schema())
      .groups.map((group) => ({
        ...group,
        fields: group.fields.filter((field) => controlFor(field) !== 'array'),
      }))
      .filter((group) => group.fields.length > 0),
  );

  /**
   * Every group with only the fields that are currently relevant given the
   * live {@link value}. Groups that lose all their fields are dropped.
   *
   * Reactivity: this reads `this.value()`, so toggling a gate field (e.g.
   * flipping `support_enabled` or changing `adhesion_type`) re-evaluates the
   * computed and makes gated fields appear/disappear live. Hidden fields keep
   * their stored values — relevance only affects rendering, never the data.
   */
  private readonly relevantGroups = computed<SchemaGroup[]>(() =>
    filterRelevantGroups(this.allGroups(), this.value()),
  );

  /**
   * Groups the panel is currently listing, honouring `visibleGroups` *and* the
   * panel's revealed tier.
   *
   * A group whose shallowest field is advanced has nothing to show in the
   * everyday view, so its header is not listed there — opening a section onto
   * an empty body is worse than not offering it. A group holding a modified
   * field is always listed whatever its tier, for the same reason a modified
   * field is always shown: the user has to be able to find what they changed.
   */
  protected readonly groups = computed<SchemaGroup[]>(() => {
    const all = this.tieredGroups();
    const visible = this.visibleGroups();
    if (!visible) {
      return orderGroupsByTier(all);
    }
    const order = new Map(visible.map((name, index) => [name, index]));
    const inTaxonomyOrder = all
      .filter((group) => order.has(group.name))
      .sort((a, b) => order.get(a.name)! - order.get(b.name)!);
    // Taxonomy order first, then tier — so revealing advanced sections adds
    // them below the everyday ones instead of slotting `Quality` in between
    // `Speed` and `Surfaces` and moving the rest of the panel down.
    return orderGroupsByTier(inTaxonomyOrder);
  });

  /**
   * How far the panel as a whole is revealed — which sections are listed.
   * Persisted, like the per-group reveal beside it.
   */
  private readonly storedPanelTier = signal<Tier>(
    this.storage.getJson<Tier>(PANEL_TIER_STORAGE_KEY, 'local') ?? 'everyday',
  );

  /**
   * Where the panel is actually revealed to: the deeper of the user's standing
   * preference and whatever they have revealed in this panel.
   *
   * The preference is a floor, not a mode — it moves where a panel *starts*, and
   * the per-section controls still open further from there.
   */
  private readonly revealedPanelTier = computed<Tier>(() =>
    deeperOf(this.storedPanelTier(), this.settingsDetail.mode()),
  );

  /**
   * Relevant groups narrowed to the ones this panel is scoped to show at all.
   *
   * The tab filter has to come *before* tiering: `Time estimate` is an
   * expert-only group on the Printer contract, and counting it while the Process
   * tab is open offered an "Expert sections 1" step that revealed nothing.
   */
  private readonly contractGroups = computed<SchemaGroup[]>(() => {
    const visible = this.visibleGroups();
    if (!visible) {
      return this.relevantGroups();
    }
    const allowed = new Set(visible);
    return this.relevantGroups().filter((g) => allowed.has(g.name));
  });

  /** Contract groups, minus the ones whose whole contents sit deeper than asked. */
  private readonly tieredGroups = computed<SchemaGroup[]>(() => {
    const revealed = this.revealedPanelTier();
    const modified = this.modifiedKeys();
    return this.contractGroups().filter(
      (group) =>
        isTierAtMost(shallowestTier(group.fields), revealed) ||
        group.fields.some((f) => modified.has(f.key)),
    );
  });

  /**
   * The tier the panel-level disclosure would reveal next, or `null` at the end.
   *
   * Walks forward to the first tier that actually reveals a section rather than
   * stopping at the immediately next one. `Time estimate` is expert-only and the
   * sole such group on the Printer tab: offering "Advanced" there would have
   * revealed nothing, and suppressing the step for that reason left the section
   * permanently unreachable.
   */
  protected readonly pendingPanelTier = computed<Tier | null>(() => {
    let candidate = nextTier(this.revealedPanelTier());
    while (candidate) {
      if (this.hiddenGroupCount(candidate) > 0) {
        return candidate;
      }
      candidate = nextTier(candidate);
    }
    return null;
  });

  /** How many more sections revealing `tier` would list. */
  protected hiddenGroupCount(tier: Tier): number {
    const shown = new Set(this.tieredGroups().map((g) => g.name));
    return this.contractGroups().filter(
      (g) => !shown.has(g.name) && isTierAtMost(shallowestTier(g.fields), tier),
    ).length;
  }

  /** Count for the template, for whichever tier is pending. */
  protected readonly pendingPanelCount = computed<number>(() => {
    const next = this.pendingPanelTier();
    return next ? this.hiddenGroupCount(next) : 0;
  });

  /** Reveal the tier the panel disclosure advertised, and remember it. */
  protected revealPanelDeeper(): void {
    const next = this.pendingPanelTier();
    if (!next) {
      return;
    }
    this.storedPanelTier.set(next);
    this.storage.writeJson(PANEL_TIER_STORAGE_KEY, next, 'local');
  }

  /** Collapse the panel back to the everyday set of sections. */
  protected hidePanelDeeper(): void {
    this.storedPanelTier.set('everyday');
    this.storage.writeJson(PANEL_TIER_STORAGE_KEY, 'everyday', 'local');
  }

  /**
   * Whether the "fewer" control can do anything.
   *
   * With a standing preference of Advanced or deeper, collapsing the panel would
   * put it straight back where it was — so the control is not offered.
   */
  protected readonly canCollapsePanel = computed(
    () => this.settingsDetail.mode() === 'everyday' && this.storedPanelTier() !== 'everyday',
  );

  /** True once the panel is showing more sections than the everyday set. */
  protected readonly panelRevealed = computed(() => this.revealedPanelTier() !== 'everyday');

  // --- Outline -----------------------------------------------------------

  /**
   * Whether the panel is showing its outline instead of its controls.
   *
   * The accordion answers "what is in this section" and search answers "where
   * is the thing I can name". Neither answers "I know this exists, I just don't
   * know what it's called" — the question a panel of several hundred settings
   * gets asked most. The outline is that third view: every section and every
   * setting name at once, dense enough to skim, with a jump behind each row.
   *
   * It is a view of the same form, not a place of its own: following a row
   * lands the user on the real control, in its real section.
   */
  protected readonly outlineOpen = signal(false);

  /** Section the form was scrolled to when the outline was opened. */
  private readonly outlineAnchor = signal<string | null>(null);
  protected readonly currentOutlineSection = this.outlineAnchor.asReadonly();

  /**
   * The contract as a table of contents, narrowed by whatever is in the box.
   *
   * Built from `contractGroups` — the *untiered* set — for the same reason
   * search reads from it: a user who cannot name a setting is exactly the user
   * disclosure has stranded, so the outline lists what the form is folding away
   * and marks the tier each row sits behind.
   */
  protected readonly outlineSections = computed<OutlineSection[]>(() =>
    filterOutline(
      buildOutline(this.contractGroups(), this.groupIcons(), this.modifiedKeys()),
      this.searchQuery(),
    ),
  );

  /**
   * Open or close the outline.
   *
   * Opening notes where the form was scrolled to, so the outline can say "you
   * are here" rather than dropping the reader at the top of a list of twelve
   * sections. It also clears the box: the same field filters the outline and
   * searches the form, and carrying a half-typed query across would narrow one
   * view by a query meant for the other.
   */
  toggleOutline(): void {
    const opening = !this.outlineOpen();
    if (opening) {
      this.sidebar?.expand();
      this.outlineAnchor.set(this.sectionInView());
    }
    this.searchQuery.set('');
    this.outlineOpen.set(opening);
  }

  /**
   * Which section the form is scrolled to, read straight from layout.
   *
   * Deliberately not an IntersectionObserver: the answer is wanted at exactly
   * one instant — when the outline opens — and an observer would run for the
   * whole session to have it ready.
   */
  private sectionInView(): string | null {
    const elements = Array.from(
      this.hostEl.nativeElement.querySelectorAll<HTMLElement>('[data-group]'),
    );
    if (elements.length === 0) {
      return null;
    }
    // Anything above the bottom of the sticky search is already scrolled past,
    // so the last such section is the one the reader is looking at.
    const anchor = (this.searchBarRef()?.nativeElement.getBoundingClientRect().bottom ?? 0) + 1;
    let current = elements[0].dataset['group'] ?? null;
    for (const element of elements) {
      if (element.getBoundingClientRect().top <= anchor) {
        current = element.dataset['group'] ?? current;
      }
    }
    return current;
  }

  /** Follow an outline section header to its section in the form. */
  protected jumpToSection(name: string): void {
    const group = this.contractGroups().find((g) => g.name === name);
    if (!group) {
      return;
    }
    this.listSection(group);
    this.expand(group.name);
    this.jumpTo(`[data-group="${CSS.escape(name)}"]`);
  }

  /** Follow an outline row to the control it names, revealing whatever hides it. */
  protected jumpToField(jump: OutlineFieldJump): void {
    const group = this.contractGroups().find((g) => g.name === jump.group);
    const field = group?.fields.find((f) => f.key === jump.key);
    if (!group || !field) {
      return;
    }
    this.listSection(group);
    this.revealField(group.name, field);
    this.expand(group.name);
    this.jumpTo(`[data-field="${CSS.escape(jump.key)}"]`);
  }

  /**
   * Make sure the panel is listing `group` at all.
   *
   * A section whose shallowest field sits behind a tier is not in the panel
   * until that tier is revealed, so a jump into one would scroll to nothing.
   */
  private listSection(group: SchemaGroup): void {
    const needed = shallowestTier(group.fields);
    if (isTierAtMost(needed, this.revealedPanelTier())) {
      return;
    }
    this.storedPanelTier.set(needed);
    this.storage.writeJson(PANEL_TIER_STORAGE_KEY, needed, 'local');
  }

  /** Reveal `group` far enough for `field` to be on screen. */
  private revealField(groupName: string, field: FieldDef): void {
    if (isFieldInTier(field, this.revealedTier(groupName))) {
      return;
    }
    this.revealedTiers.update((map) => ({ ...map, [groupName]: tierOf(field) }));
    this.storage.writeJson(TIER_STORAGE_KEY, this.revealedTiers(), 'local');
  }

  private expand(groupName: string): void {
    this.getExpandedSignal(groupName).set(true);
    this.persistExpandedState();
  }

  /** Leave the outline and bring `selector` into view. */
  private jumpTo(selector: string): void {
    this.outlineOpen.set(false);
    this.searchQuery.set('');
    setTimeout(() => this.scrollToTarget(selector), 0);
  }

  private scrollToTarget(selector: string, attempt = 0): void {
    const element = this.hostEl.nativeElement.querySelector<HTMLElement>(selector);
    if (!element) {
      if (attempt < JUMP_RETRIES) {
        setTimeout(() => this.scrollToTarget(selector, attempt + 1), JUMP_RETRY_MS);
      }
      return;
    }
    // `scroll-margin-top` on the target clears the sticky search bar, so
    // `start` lands the row under the chrome rather than behind it.
    element.scrollIntoView({ behavior: 'smooth', block: 'start' });
    this.flash(element);
  }

  /**
   * Mark where a jump landed, briefly.
   *
   * A smooth scroll ends with the target somewhere on a panel of near-identical
   * rows, and the row the user asked for looks like every other one.
   */
  private flash(element: HTMLElement): void {
    element.classList.remove(JUMP_CLASS);
    // Force a reflow so jumping to the same row twice restarts the animation
    // instead of being folded into the one already running.
    void element.offsetWidth;
    element.classList.add(JUMP_CLASS);
    setTimeout(() => element.classList.remove(JUMP_CLASS), JUMP_FLASH_MS);
  }

  /**
   * All currently-relevant fields flattened with their group name, used to
   * build the Fuse index. Hidden (gated-off) fields are excluded so they do
   * not surface in search results while their gate condition is unmet.
   *
   * **Deliberately not tier-filtered.** Search is the escape hatch that makes a
   * calm default view affordable: someone who knows the term types it and lands
   * on the control wherever it sits in the taxonomy. A tier governs what is
   * shown before the user asks — a tier that hid a setting from search would
   * have stopped being disclosure and become a feature flag. `relevantGroups`
   * is the untiered set, and this must keep reading from it.
   */
  private readonly flatFields = computed<FieldDefIndexed[]>(() =>
    this.relevantGroups().flatMap((g) => g.fields.map((f) => ({ ...f, groupName: g.name }))),
  );

  /**
   * Names of groups that currently contain at least one field with an active
   * {@link noticeForField} exception (given the live values). Drives the neutral
   * "double-check this section" hint on the accordion header, so a collapsed
   * group still advertises that something inside wants a second look.
   * Only relevant (visible) fields are considered.
   */
  protected readonly groupsWithNotice = computed<ReadonlySet<string>>(() => {
    const values = this.value();
    const names = new Set<string>();
    for (const group of this.relevantGroups()) {
      if (group.fields.some((f) => noticeForField(f, values[f.key], values) !== null)) {
        names.add(group.name);
      }
    }
    return names;
  });

  /**
   * Groups holding at least one currently-visible modified field, so a
   * collapsed section still advertises that something inside was changed.
   */
  protected readonly groupsWithModified = computed<ReadonlySet<string>>(() => {
    const modified = this.modifiedKeys();
    if (modified.size === 0) {
      return new Set<string>();
    }
    const names = new Set<string>();
    for (const group of this.relevantGroups()) {
      if (group.fields.some((f) => modified.has(f.key))) {
        names.add(group.name);
      }
    }
    return names;
  });

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
   * How far each group is currently revealed. Missing means `everyday`.
   *
   * Read from storage once and then held here, so a reveal survives a reload
   * without the template touching storage on every change detection.
   */
  private readonly revealedTiers = signal<Record<string, Tier>>(
    this.storage.getJson<Record<string, Tier>>(TIER_STORAGE_KEY, 'local') ?? {},
  );

  /** How far `groupName` is revealed right now. */
  protected revealedTier(groupName: string): Tier {
    // Never shallower than the panel: a section that only exists because the
    // user revealed Advanced must show its advanced fields, not an empty body.
    // The panel tier already folds in the standing preference, so a user who
    // works at Expert gets every section open at Expert without touching one.
    return deeperOf(this.revealedTiers()[groupName] ?? 'everyday', this.revealedPanelTier());
  }

  /**
   * Fields of `group` that should be on screen, given how far it is revealed.
   *
   * A *modified* field is always shown whatever its tier. Hiding a value the
   * user has already changed is the one disclosure failure that cannot be
   * argued for: they cannot put it back if they cannot find it, and the group
   * header's "changed" dot would point into an empty section.
   */
  protected visibleFields(group: SchemaGroup): FieldDef[] {
    const revealed = this.revealedTier(group.name);
    const modified = this.modifiedKeys();
    return group.fields.filter((f) => isFieldInTier(f, revealed) || modified.has(f.key));
  }

  /**
   * The tier the disclosure below a group would reveal next, or `null` when
   * there is nothing deeper to show.
   */
  protected pendingTier(group: SchemaGroup): Tier | null {
    const deepest = deepestTier(group.fields);
    let candidate = nextTier(this.revealedTier(group.name));
    // Walk to the first step that actually fills. A section whose extra fields
    // are all expert must offer "Expert" directly rather than an "Advanced"
    // step that expands to nothing — and one whose extras are all advanced must
    // not advertise an Expert tier at all.
    while (candidate && TIER_RANK[candidate] <= TIER_RANK[deepest]) {
      if (this.countInTier(group, candidate) > 0) {
        return candidate;
      }
      candidate = nextTier(candidate);
    }
    return null;
  }

  /** Fields of `group` that revealing `tier` would newly bring into view. */
  private countInTier(group: SchemaGroup, tier: Tier): number {
    const shown = new Set(this.visibleFields(group).map((f) => f.key));
    return group.fields.filter((f) => !shown.has(f.key) && isFieldInTier(f, tier)).length;
  }

  /** How many more fields the pending disclosure would bring into view. */
  protected pendingCount(group: SchemaGroup): number {
    const next = this.pendingTier(group);
    return next ? this.countInTier(group, next) : 0;
  }

  /**
   * Reveal the tier the disclosure advertised, and remember it.
   *
   * Takes the tier from `pendingTier` rather than stepping one place: where the
   * next step would reveal nothing, the button says "Expert" and pressing it has
   * to land on Expert. Stepping blindly meant the first press appeared to do
   * nothing at all.
   */
  protected revealDeeper(group: SchemaGroup): void {
    const next = this.pendingTier(group);
    if (!next) {
      return;
    }
    this.revealedTiers.update((map) => ({ ...map, [group.name]: next }));
    this.storage.writeJson(TIER_STORAGE_KEY, this.revealedTiers(), 'local');
  }

  /**
   * Whether collapsing `groupName` would change anything.
   *
   * With a standing preference of Advanced or deeper, a section cannot go below
   * that floor — offering "Show less" there is a control that does nothing when
   * pressed.
   */
  protected canCollapseGroup(groupName: string): boolean {
    const own = this.revealedTiers()[groupName] ?? 'everyday';
    return own !== 'everyday' && own !== this.settingsDetail.mode();
  }

  /** Collapse `groupName` back to the everyday view. */
  protected hideDeeper(groupName: string): void {
    this.revealedTiers.update((map) => {
      const next = { ...map };
      delete next[groupName];
      return next;
    });
    this.storage.writeJson(TIER_STORAGE_KEY, this.revealedTiers(), 'local');
  }

  /** Label for the disclosure control under a group. */
  protected tierLabel(tier: Tier): string {
    return tier === 'advanced' ? 'Advanced' : 'Expert';
  }

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

  /**
   * The value to render for `field`, falling back to the schema's own default
   * when the settings object does not carry the key.
   *
   * The engine fills an absent field in from the same default (`#[serde(default
   * = …)]`), so showing blank-as-`false` told the user the opposite of what the
   * slice would do — `support_auto` defaults to on, and its switch read off
   * until someone touched it. This is display only: a key the user never set
   * stays absent, so the sparse override diff sent to the engine is unchanged.
   */
  protected valueFor(field: FieldDef): unknown {
    const current = this.value()[field.key];
    return current === undefined || current === null ? field.default : current;
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
