import type { InlineNoticeTone } from '../../ui/inline-notice/inline-notice';
import type { FieldDef } from '../models/field-def';

/**
 * An in-app link rendered at the end of a {@link FieldNotice}, for pointing the
 * user at the place that fixes what the notice is about (e.g. a setting that
 * lives on a different profile / tab).
 */
export interface FieldNoticeLink {
  /** Visible link text. */
  text: string;
  /** Angular router path, e.g. `'/settings/printers'`. */
  routerLink: string;
}

/**
 * A contextual notice rendered directly beneath a single field's control.
 */
export interface FieldNotice {
  /** Severity; defaults to `'info'`. */
  tone?: InlineNoticeTone;
  /** Optional icon-name override; defaults to the tone's icon. */
  icon?: string;
  /** Optional bold lead line. */
  title?: string;
  /** Body text. */
  text: string;
  /** Optional trailing link to wherever the notice can be acted on. */
  link?: FieldNoticeLink;
}

/**
 * Per-field special-casing that layers **on top of** the widget registry.
 *
 * The widget registry ([`resolveWidget`](../field-registry/field-registry.ts))
 * answers "which control renders this field" and stays deliberately generic.
 * This registry answers the orthogonal question "does this specific field need
 * extra treatment" — today only a conditional {@link FieldNotice}, but the shape
 * leaves room for future exception kinds without touching the widget-choosing
 * path. Keeping the two concerns in separate maps is what stops either from
 * accreting special cases.
 */
export interface FieldException {
  /**
   * Produce a notice to show beneath the field for its current `value`, or
   * `null` for none.
   *
   * `siblings` carries every other value in scope, so an exception can express a
   * **cross-field** condition — including one that spans profile contracts. The
   * profile editors merge the active printer's params into it precisely so a
   * filament setting can ask about the machine it will run on (see
   * `ui-design-language.instructions.md`, "Cross-contract dependencies").
   * Treat a missing key as "unknown", not as `false`.
   */
  notice?(value: unknown, siblings: Readonly<Record<string, unknown>>): FieldNotice | null;
}

/**
 * Field-key → exception. Add an entry to give a specific setting bespoke
 * treatment; every other field is unaffected.
 */
export const FIELD_EXCEPTIONS: Record<string, FieldException> = {
  // Raft shifts the model up in Z (raft height + air gap) and prints a base
  // beneath it, so the sliced G-code no longer lines up with the object preview.
  adhesion_type: {
    notice: (value) =>
      value === 'raft'
        ? {
            tone: 'warning',
            title: 'Sliced result will differ from the object preview',
            text:
              'A raft prints a sacrificial base under the model and lifts it by the raft ' +
              'height plus the air gap, so the model sits higher in the sliced G-code ' +
              'preview than it does in the object view.',
          }
        : null,
  },

  // The chamber target belongs to the filament; the chamber *heater* belongs to
  // the printer. Without the machine capability the slicer emits no chamber
  // command at all — deliberately, since an unknown command aborts the print on
  // Klipper — so an unheeded chamber temperature is indistinguishable from a
  // heeded one until the part warps. Say it, and link to the switch.
  chamber_temp: {
    notice: (value, siblings) => chamberWithoutHeaterNotice(value, siblings),
  },
  chamber_temp_first_layer: {
    notice: (value, siblings) => chamberWithoutHeaterNotice(value, siblings),
  },
};

/**
 * Shared body for the two chamber-temperature fields: warn when a real target is
 * set on a machine that has not been told it can heat a chamber.
 *
 * `heated_chamber` absent means the printer profile predates the setting rather
 * than opting out, so it is treated as unknown and left un-warned — a notice the
 * user cannot act on is just noise.
 */
function chamberWithoutHeaterNotice(
  value: unknown,
  siblings: Readonly<Record<string, unknown>>,
): FieldNotice | null {
  const wantsChamber = typeof value === 'number' && value > 0;
  if (!wantsChamber || siblings['heated_chamber'] !== false) {
    return null;
  }
  return {
    tone: 'warning',
    title: 'Your printer is not set up to heat a chamber',
    text:
      'No chamber command will be emitted, and the chamber will stay at room temperature. ' +
      'The slicer only heats a chamber when the printer profile says the machine has a ' +
      'heater, because an unknown chamber command aborts the print on Klipper. Your start ' +
      'G-code can still read this value as {chamber_temp}.',
    link: {
      text: 'Turn on Heated Chamber in printer settings',
      routerLink: '/settings/printers',
    },
  };
}

/**
 * Resolve the notice (if any) that applies to `field` at its current `value`.
 *
 * `siblings` defaults to empty, which makes every cross-field condition evaluate
 * to "unknown" and stay silent — the safe default for a caller that has no
 * surrounding values to offer.
 */
export function noticeForField(
  field: FieldDef,
  value: unknown,
  siblings: Readonly<Record<string, unknown>> = {},
): FieldNotice | null {
  return FIELD_EXCEPTIONS[field.key]?.notice?.(value, siblings) ?? null;
}
