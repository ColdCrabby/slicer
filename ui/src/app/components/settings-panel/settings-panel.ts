import {
  Component,
  ElementRef,
  afterRenderEffect,
  computed,
  effect,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { RouterLink } from '@angular/router';
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import {
  bucketGroupsByContract,
  GROUP_ICONS,
  SETTING_CONTRACTS,
  type SettingContractId,
} from '../../models/setting-contract';
import { parseSchema } from '../../schema-form/models/schema-parser';
import { FieldChangeEvent, SchemaForm } from '../../schema-form/schema-form';
import { BrowserStorage } from '../../services/browser-storage';
import { ActivePresets } from '../../services/profiles/active-presets';
import { ActiveSelection } from '../../services/profiles/active-selection';
import { LabelFilterStore } from '../../services/profiles/label-filter-store';
import { ProfileWriteback } from '../../services/profiles/profile-writeback';
import { LabelFilterBar } from '../labels/label-filter-bar';
import { WritebackDialog } from '../writeback-dialog/writeback-dialog';
import { Dialog } from '../../services/dialog';
import { Slicer } from '../../services/slicer';
import {
  WORKPLATE_SAVE_DEBOUNCE_MS,
  WorkplateSettingsStore,
  type WorkplateSaveStatus,
} from '../../services/workplate-settings';
import { Icon, IconButton, Select, TooltipDirective } from '@coldcrabby/ui';

// Extract the SlicingParams sub-schema so the form renders all slicer settings.
// (`SlicingParams` is now the wire-format type — the legacy `WsSlicingParams`
// has been collapsed into a Rust type alias for it, so every form field
// reaches the slicer pipeline as-is.)
const SLICING_PARAMS_SCHEMA = {
  ...(globalSettingsSchema.$defs.SlicingParams as Record<string, unknown>),
  $defs: globalSettingsSchema.$defs as Record<string, unknown>,
};

const CONTRACT_STORAGE_KEY = 'settings-panel.contract';

/** How long an armed "Reset all" waits before disarming itself. */
const CONFIRM_TIMEOUT_MS = 4000;

/**
 * Slice-page sidebar. Categorises the flat slicer parameters by *contract*
 * (Printer / Filament / Process) the way established slicers do: a tab switches
 * contract, a preset dropdown selects the active printer / filament / print
 * profile for that contract, and the schema form below shows only that
 * contract's parameter groups. Global settings search still spans everything.
 */
@Component({
  selector: 'nexus-settings-panel',
  standalone: true,
  imports: [SchemaForm, Select, Icon, IconButton, RouterLink, LabelFilterBar, TooltipDirective],
  templateUrl: './settings-panel.component.html',
  styleUrl: './settings-panel.component.scss',
})
export class SettingsPanel {
  private readonly slicer = inject(Slicer);
  private readonly storage = inject(BrowserStorage);
  private readonly workplateSettings = inject(WorkplateSettingsStore);
  private readonly dialog = inject(Dialog);
  private readonly writeback = inject(ProfileWriteback);
  protected readonly presets = inject(ActivePresets);
  private readonly activeSelection = inject(ActiveSelection);
  protected readonly labelFilter = inject(LabelFilterStore);

  readonly settings = this.slicer.settings;
  readonly schema = SLICING_PARAMS_SCHEMA;
  protected readonly groupIcons = GROUP_ICONS;

  /**
   * Settings this plate deviates from its presets on — exactly the diff that
   * goes on the wire. Marking them is what keeps the panel honest: every other
   * value on screen is inherited and will follow its profile if that profile
   * is edited.
   */
  protected readonly modifiedKeys = this.slicer.overriddenKeys;
  protected readonly modifiedCount = computed(() => this.modifiedKeys().size);

  /**
   * Settings this machine corrects for the active material, and the sentence
   * naming that correction ("Voron 2.4 · PLA").
   *
   * These are the one layer that can disagree with the profile the user picked
   * while still being right, so they are the one layer worth pointing at. The
   * other four are visible in the panel already: a modified key is marked, and
   * everything else is whatever the selected presets say.
   */
  protected readonly machineMaterialKeys = computed(() => {
    const origins = this.activeSelection.paramOrigins();
    const keys = new Set<string>();
    for (const [key, origin] of origins) {
      if (origin === 'machine_material' && !this.modifiedKeys().has(key)) {
        keys.add(key);
      }
    }
    return keys as ReadonlySet<string>;
  });
  protected readonly machineMaterialLabel = this.activeSelection.materialOverlayLabel;

  /**
   * What the sync button says, in both states.
   *
   * It is always on screen, so it has to explain its own quiet state rather
   * than leaving a dimmed icon to be guessed at.
   */
  protected readonly syncTooltip = computed(() => {
    const count = this.modifiedCount();
    if (count === 0) {
      return 'No changed settings to sync into your profiles';
    }
    return `Sync ${count} changed ${count === 1 ? 'setting' : 'settings'} to their profiles`;
  });

  /** All three contracts, in tab order — the plate's recipe, top to bottom. */
  protected readonly contracts = SETTING_CONTRACTS;

  protected presetOptionsFor(contract: SettingContractId) {
    return this.presets.options(contract);
  }

  protected selectedIdFor(contract: SettingContractId): string | null {
    return this.presets.selectedId(contract);
  }

  /** The preset a row is showing, or an invitation to make one. */
  protected presetNameFor(contract: SettingContractId): string {
    const id = this.presets.selectedId(contract);
    const meta = SETTING_CONTRACTS.find((c) => c.id === contract)!;
    return (
      this.presets.options(contract).find((option) => option.value === id)?.label ??
      `Add a ${meta.label.toLowerCase()} preset…`
    );
  }

  protected selectPresetFor(contract: SettingContractId, id: string): void {
    this.presets.select(contract, id);
  }

  /**
   * Open that row's preset in its own editor, scrolled to it — the same
   * hand-off a wizard's "Add & configure" uses.
   */
  protected editParamsFor(contract: SettingContractId): Record<string, string> {
    const id = this.presets.selectedId(contract);
    return id ? { configure: id } : {};
  }

  protected editLabelFor(contract: SettingContractId): string {
    const meta = SETTING_CONTRACTS.find((c) => c.id === contract)!;
    const name = this.presets
      .options(contract)
      .find((option) => option.value === this.presets.selectedId(contract))?.label;
    return name ? `Edit ${name}` : `Manage ${meta.label} presets`;
  }

  protected readonly activeContract = signal<SettingContractId>(
    this.storage.getJson<SettingContractId>(CONTRACT_STORAGE_KEY, 'local') ?? 'process',
  );

  protected readonly activeContractMeta = computed(() =>
    SETTING_CONTRACTS.find((c) => c.id === this.activeContract())!,
  );

  private readonly groupsByContract = computed(() =>
    bucketGroupsByContract(parseSchema(this.schema).groups.map((g) => g.name)),
  );

  /** Group names shown for the active contract. */
  protected readonly activeGroups = computed(() => this.groupsByContract()[this.activeContract()]);

  setContract(id: string): void {
    this.activeContract.set(id as SettingContractId);
    this.storage.writeJson(CONTRACT_STORAGE_KEY, id, 'local');
  }

  update(event: FieldChangeEvent): void {
    this.slicer.updateSettings({ [event.key]: event.value });
  }

  /**
   * Two-step: arm on the first press, act on the second. Dropping every
   * override at once can undo a long evening of tuning, and there is no undo
   * stack behind it.
   */
  protected readonly resetConfirming = signal(false);
  private resetTimer: ReturnType<typeof setTimeout> | null = null;

  /** Hand every changed setting back to the presets it came from. */
  resetOverrides(): void {
    if (!this.resetConfirming()) {
      this.resetConfirming.set(true);
      this.resetTimer = setTimeout(() => this.cancelReset(), CONFIRM_TIMEOUT_MS);
      return;
    }
    this.cancelReset();
    this.slicer.resetAllSettings();
  }

  /** Disarm the confirm — on blur, or when it has sat untouched long enough. */
  cancelReset(): void {
    if (this.resetTimer !== null) {
      clearTimeout(this.resetTimer);
      this.resetTimer = null;
    }
    this.resetConfirming.set(false);
  }

  /**
   * Review every changed setting against a git-diff-style dialog and write the
   * accepted ones back into the printer / filament / process profile that owns
   * them — without leaving the slice page to open the profile editors.
   */
  syncToProfile(): void {
    this.writeback.open();
    this.dialog
      .confirm({
        title: 'Sync changes to your profiles',
        message: 'Check off which changed settings should become part of their profile.',
        confirmLabel: 'Sync checked settings',
        cancelLabel: "Don't sync",
        content: WritebackDialog,
        preferredWidth: '560px',
      })
      .subscribe((confirmed) => {
        if (confirmed) {
          this.writeback.apply();
        }
      });
  }

  // --- Save indicator ----------------------------------------------------

  /**
   * Whether to show the indicator. Held back by the save debounce so an edit
   * that settles inside that window never flashes it, and dropped immediately
   * once the store goes idle — the same restraint the Settings sidebar shows.
   */
  protected readonly saveVisible = signal(false);

  /** Status being displayed; held through the fade so the label never blanks. */
  private readonly shownStatus = signal<WorkplateSaveStatus>('idle');

  protected readonly saveLabel = computed(() => {
    switch (this.shownStatus()) {
      case 'pending':
      case 'saving':
        return 'Saving…';
      case 'saved':
        return 'Saved to this workplate';
      case 'error':
        return "Couldn't save";
      default:
        return '';
    }
  });

  protected readonly saveIsError = computed(() => this.shownStatus() === 'error');

  /**
   * The footer says one thing at a time, in one row.
   *
   * A save is transient and a change count is not, so the save takes the slot
   * while it lasts and the count comes back underneath it. Giving them a line
   * each is what made the strip flicker: the save line appeared and vanished on
   * every edit, resizing the footer and shifting the panel above it.
   */
  protected readonly footerLabel = computed(() => {
    if (this.saveVisible()) {
      return this.saveLabel();
    }
    const changed = this.modifiedCount();
    // Short, because the sidebar is narrow and this shares its row with the
    // reset button; the sentence it stands for is the tooltip.
    return `${changed} ${changed === 1 ? 'setting' : 'settings'} changed`;
  });

  /** The full sentence, for the row's tooltip. */
  protected readonly footerTitle = computed(() =>
    this.saveVisible() ? this.saveLabel() : `${this.footerLabel()} from your presets`,
  );

  /** Nothing to report means no strip at all, rather than an empty one. */
  protected readonly footerVisible = computed(() => this.saveVisible() || this.modifiedCount() > 0);

  private readonly footerRef = viewChild<ElementRef<HTMLElement>>('footer');

  constructor() {
    // Publish the footer's height so anything floating at the bottom of the
    // sidebar can clear it. The sidebar's "scroll to top" button is positioned
    // from that same edge and has a lower stacking order, so before this it was
    // simply drawn behind the footer — visible as a half-circle poking out of
    // the top of the strip. The footer comes and goes, and changes height with
    // the text, so the value is measured rather than assumed; `0px` when there
    // is no footer keeps the button where it has always been.
    let observer: ResizeObserver | null = null;
    afterRenderEffect({
      read: (onCleanup) => {
        const el = this.footerRef()?.nativeElement;
        observer?.disconnect();
        observer = null;

        const clear = () => document.documentElement.style.removeProperty('--settings-footer-h');
        if (!el) {
          clear();
          return;
        }

        observer = new ResizeObserver(() => {
          document.documentElement.style.setProperty(
            '--settings-footer-h',
            `${Math.round(el.offsetHeight)}px`,
          );
        });
        observer.observe(el);

        onCleanup(() => {
          observer?.disconnect();
          observer = null;
          clear();
        });
      },
    });

    effect((onCleanup) => {
      const status = this.workplateSettings.status();
      if (status === 'idle') {
        this.saveVisible.set(false);
        return;
      }
      this.shownStatus.set(status);
      // Only `pending` waits: it is the one state that resolves on its own, and
      // an edit that settles inside the debounce should never have flashed an
      // indicator at all. Everything past it reports a write that has already
      // happened, so it appears at once — the store retires `saved` itself.
      if (status !== 'pending') {
        this.saveVisible.set(true);
        return;
      }
      const timer = setTimeout(() => this.saveVisible.set(true), WORKPLATE_SAVE_DEBOUNCE_MS);
      onCleanup(() => clearTimeout(timer));
    });
  }
}
