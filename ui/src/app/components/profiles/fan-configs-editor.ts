import { ChangeDetectionStrategy, Component, computed, input, output, signal } from '@angular/core';
import type { FanConfig } from '../../../generated/slicer-engine-global-settings-v1';
import {
  Icon,
  Button,
  IconButton,
  FieldRow,
  NumberInput,
  Select,
  type SelectOption,
} from '@coldcrabby/ui';

/**
 * A hardware fan role → its default Klipper object name + Marlin `M106 P<n>`
 * index. These cover the common cases; any other physical fan is handled by
 * typing a custom Klipper name against the closest role.
 */
interface FanRolePreset {
  index: number;
  label: string;
  /** Klipper fan object name the engine derives when no override is set. */
  defaultName: string;
}

export const FAN_ROLE_PRESETS: readonly FanRolePreset[] = [
  { index: 0, label: 'Part cooling', defaultName: 'fan' },
  { index: 1, label: 'Hotend', defaultName: 'fan_hotend' },
  { index: 2, label: 'Chamber', defaultName: 'fan_chamber' },
  { index: 3, label: 'Auxiliary', defaultName: 'fan_aux' },
];

const REMOVE_CONFIRM_TIMEOUT_MS = 3000;

/** A sane fan configuration for a freshly-added row of the given role index. */
function defaultFan(index: number): FanConfig {
  return {
    fan_index: index,
    min_speed: 0.35,
    max_speed: 1.0,
    layer_time_fast_s: 10,
    layer_time_slow_s: 30,
  };
}

/**
 * Editor for the `fan_configs` array — the per-fan adaptive cooling table the
 * engine turns into `M106`/`SET_FAN_SPEED` commands at slice time.
 *
 * Purely presentational: the parent owns the array and persists it. Each row
 * maps one physical fan (a role/index plus an optional Klipper object name) to
 * a layer-time cooling curve, letting a user drive *any* named fan the printer
 * hardware exposes — e.g. a Klipper `[fan_generic rscs]` or `exhaust_filter`.
 *
 * The part-cooling fan (index 0, no name) is emitted as Marlin-compatible
 * `M106`/`M107`; a named fan (or any non-zero role) uses Klipper's
 * `SET_FAN_SPEED fan=<name>`.
 */
@Component({
  selector: 'nexus-fan-configs-editor',
  standalone: true,
  imports: [Icon, Button, IconButton, FieldRow, NumberInput, Select],
  templateUrl: './fan-configs-editor.html',
  styleUrl: './fan-configs-editor.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FanConfigsEditor {
  readonly configs = input<readonly FanConfig[]>([]);
  readonly configsChange = output<FanConfig[]>();

  protected readonly roleOptions: readonly SelectOption[] = FAN_ROLE_PRESETS.map((r) => ({
    value: String(r.index),
    label: r.label,
  }));

  protected readonly rows = computed(() => this.configs() ?? []);
  protected readonly confirmRemoveIndex = signal<number | null>(null);
  private removeConfirmTimer: ReturnType<typeof setTimeout> | null = null;

  /** The `<nexus-select>` value for a role index (options are string-keyed). */
  protected roleValue(index: number): string {
    return String(index);
  }

  /** Human label for a role index (falls back to "Auxiliary" for custom indices). */
  protected roleLabel(index: number): string {
    return FAN_ROLE_PRESETS.find((r) => r.index === index)?.label ?? 'Auxiliary';
  }

  /** Placeholder shown in the name field: the engine's derived default name. */
  protected defaultName(index: number): string {
    return FAN_ROLE_PRESETS.find((r) => r.index === index)?.defaultName ?? 'fan_aux';
  }

  protected addFan(): void {
    this.clearRemoveFanConfirm();
    const rows = this.rows();
    // First fan is the part-cooling fan; subsequent ones default to auxiliary.
    const index = rows.length === 0 ? 0 : 3;
    this.configsChange.emit([...rows, defaultFan(index)]);
  }

  protected requestRemoveFan(i: number): void {
    if (this.confirmRemoveIndex() === i) {
      this.removeFan(i);
      return;
    }
    this.armRemoveFan(i);
  }

  protected removeFan(i: number): void {
    this.clearRemoveFanConfirm();
    this.configsChange.emit(this.rows().filter((_, idx) => idx !== i));
  }

  /** Immutable per-row patch that preserves untouched fields (e.g. aux_overrides). */
  private patch(i: number, patch: Partial<FanConfig>): void {
    this.configsChange.emit(this.rows().map((c, idx) => (idx === i ? { ...c, ...patch } : c)));
  }

  protected setRole(i: number, value: string): void {
    this.patch(i, { fan_index: Number(value) });
  }

  protected setName(i: number, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    this.patch(i, { klipper_name: name === '' ? null : name });
  }

  protected setMinSpeed(i: number, pct: number): void {
    this.patch(i, { min_speed: clamp01(pct / 100) });
  }

  protected setMaxSpeed(i: number, pct: number): void {
    this.patch(i, { max_speed: clamp01(pct / 100) });
  }

  protected setFast(i: number, seconds: number): void {
    this.patch(i, { layer_time_fast_s: Math.max(0, seconds) });
  }

  protected setSlow(i: number, seconds: number): void {
    this.patch(i, { layer_time_slow_s: Math.max(0, seconds) });
  }

  protected pct(fraction: number): number {
    return Math.round(clamp01(fraction) * 100);
  }

  private armRemoveFan(i: number): void {
    this.clearRemoveFanConfirm();
    this.confirmRemoveIndex.set(i);
    this.removeConfirmTimer = setTimeout(() => {
      if (this.confirmRemoveIndex() === i) {
        this.confirmRemoveIndex.set(null);
      }
      this.removeConfirmTimer = null;
    }, REMOVE_CONFIRM_TIMEOUT_MS);
  }

  private clearRemoveFanConfirm(): void {
    if (this.removeConfirmTimer !== null) {
      clearTimeout(this.removeConfirmTimer);
      this.removeConfirmTimer = null;
    }
    this.confirmRemoveIndex.set(null);
  }
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}
