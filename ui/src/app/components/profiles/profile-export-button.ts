import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import type { ElementRef, TemplateRef } from '@angular/core';
import { ProfileExport } from '../../services/profiles/profile-export';
import type { ProfileExportFormat } from '../../services/profiles/profile-persistence';
import { FloatingService, type FloatingRef, Icon } from '@coldcrabby/ui';

/** One row of the export menu. */
interface ExportOption {
  value: ProfileExportFormat;
  label: string;
  description: string;
  icon: string;
}

/**
 * Split button that downloads the profile library.
 *
 * The primary button exports the **bundle** — a ZIP with one TOML file per
 * profile, which is what most people want (readable, shareable, one file per
 * thing). The caret opens the alternative: a single `profiles.toml`, the exact
 * file the engine and CLI read, for dropping straight into a config directory.
 *
 * Dumb apart from one injected service: it renders two choices and calls
 * {@link ProfileExport}. The list is fixed because there are exactly two file
 * *shapes* a library can take — new profile settings change neither.
 */
@Component({
  selector: 'nexus-profile-export-button',
  imports: [Icon],
  templateUrl: './profile-export-button.html',
  styleUrl: './profile-export-button.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ProfileExportButton {
  protected readonly exporter = inject(ProfileExport);
  private readonly floating = inject(FloatingService);

  private readonly caretEl = viewChild<ElementRef<HTMLElement>>('caret');
  private readonly menuTpl = viewChild<TemplateRef<unknown>>('menuTpl');
  private menuRef: FloatingRef | null = null;

  protected readonly menuOpen = signal(false);

  protected readonly options: ExportOption[] = [
    {
      value: 'bundle',
      label: 'Bundle (separate files)',
      description: 'ZIP with one TOML per printer, filament and profile',
      icon: 'box-iso',
    },
    {
      value: 'toml',
      label: 'CLI configuration',
      description: 'A single profiles.toml for the slicer config directory',
      icon: 'developer',
    },
  ];

  constructor() {
    inject(DestroyRef).onDestroy(() => this.closeMenu());
  }

  /** The primary action: the separate-files bundle. */
  protected exportBundle(): void {
    void this.exporter.export('bundle');
  }

  protected pick(format: ProfileExportFormat): void {
    this.closeMenu();
    void this.exporter.export(format);
  }

  protected toggleMenu(): void {
    this.menuOpen() ? this.closeMenu() : this.openMenu();
  }

  private openMenu(): void {
    const trigger = this.caretEl()?.nativeElement;
    const tpl = this.menuTpl();
    if (!trigger || !tpl) {
      return;
    }
    this.menuOpen.set(true);
    this.menuRef = this.floating.openTemplate(
      tpl,
      {},
      {
        reference: trigger,
        interactive: true,
        panelClass: 'nexus-floating--fit',
        originElement: trigger,
        options: { placement: 'bottom-end', offset: 4, padding: 8 },
        onOutsidePointer: () => this.closeMenu(),
        onEscape: () => this.closeMenu(),
      },
    );
  }

  private closeMenu(): void {
    this.menuOpen.set(false);
    this.menuRef?.close();
    this.menuRef = null;
  }
}
