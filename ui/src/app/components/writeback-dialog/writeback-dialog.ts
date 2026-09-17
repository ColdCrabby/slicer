import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { Icon } from '@coldcrabby/ui';
import { SETTING_CONTRACTS, type SettingContractId } from '../../models/setting-contract';
import { enumLabel } from '../../schema-form/models/field-labels';
import { ProfileWriteback, type WritebackRow } from '../../services/profiles/profile-writeback';

/** Render a stored setting value the same plain way on both sides of a row. */
function formatValue(value: unknown): string {
  if (value === undefined || value === null) {
    return '—';
  }
  if (typeof value === 'boolean') {
    return value ? 'On' : 'Off';
  }
  if (typeof value === 'string') {
    return enumLabel(value);
  }
  if (typeof value === 'object') {
    return JSON.stringify(value);
  }
  return String(value);
}

/** Whether a value needs the block diff layout rather than one inline row. */
function isMultiline(value: unknown): boolean {
  return typeof value === 'string' && value.includes('\n');
}

/** One contract's section of the review list. */
interface WritebackSection {
  contract: SettingContractId;
  label: string;
  icon: string;
  rows: WritebackRow[];
}

/**
 * Body of the "write changes back to your profiles" dialog: every setting the
 * current plate deviates from its presets on, grouped by the printer /
 * filament / process profile that owns it — the same three contracts the
 * settings sidebar tabs between — with the preset value and the plate's
 * override side by side and an accept/reject choice on each row before the
 * dialog's Confirm button hands the accepted rows to {@link ProfileWriteback}.
 *
 * Rendered by the {@link Dialog} service via `NgComponentOutlet`. Like
 * `OperationPipelineDialog`, it pulls its data from an injected service rather
 * than inputs — here {@link ProfileWriteback}, which the "Sync to profile"
 * trigger also holds so it can call `apply()` once the dialog resolves.
 */
@Component({
  selector: 'nexus-writeback-dialog',
  standalone: true,
  imports: [Icon],
  templateUrl: './writeback-dialog.html',
  styleUrl: './writeback-dialog.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WritebackDialog {
  protected readonly writeback = inject(ProfileWriteback);
  protected readonly formatValue = formatValue;
  protected readonly isMultiline = isMultiline;

  /** Rows bucketed into their contract, in the sidebar's own tab order. */
  protected readonly sections = computed<WritebackSection[]>(() => {
    const rows = this.writeback.rows();
    return SETTING_CONTRACTS.map((contract) => ({
      contract: contract.id,
      label: contract.label,
      icon: contract.icon,
      rows: rows.filter((row) => row.contract === contract.id),
    })).filter((section) => section.rows.length > 0);
  });

  /** A multi-line value rendered as unified-diff-style `−`/`+` lines. */
  protected diffLines(value: unknown, marker: '−' | '+'): string {
    return formatValue(value)
      .split('\n')
      .map((line) => `${marker} ${line}`)
      .join('\n');
  }

  /**
   * The profile a row would be written to if it is not saved for this machine —
   * named, so the wider choice states what it actually changes.
   */
  protected profileTargetLabel(row: WritebackRow): string {
    switch (row.contract) {
      case 'printer':
        return this.writeback.printerName();
      case 'filament':
        return this.writeback.filamentName();
      case 'process':
        return this.writeback.processName();
    }
  }

  protected accept(row: WritebackRow): void {
    this.writeback.setAccepted(row.key, true);
  }

  protected reject(row: WritebackRow): void {
    this.writeback.setAccepted(row.key, false);
  }

  /** Accept or reject every row in one contract's section at once. */
  protected setSection(section: WritebackSection, value: boolean, event: Event): void {
    // The buttons live inside a <summary>, whose native click toggles the
    // accordion — stopped here so a bulk choice doesn't also collapse the
    // section the user is looking at.
    event.preventDefault();
    event.stopPropagation();
    for (const row of section.rows) {
      this.writeback.setAccepted(row.key, value);
    }
  }
}
