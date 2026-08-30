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
   * `null` for none. `value` is the field's own value; wire in siblings here if
   * a future exception needs cross-field conditions.
   */
  notice?(value: unknown): FieldNotice | null;
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

  // Printing objects one at a time is only safe if the printhead can clear
  // everything already on the bed — which is a property of the *machine*, not
  // this process. Be honest that the guarantee lives elsewhere and link the
  // user straight to where the clearances are set, rather than letting them
  // discover the dependency only when the slicer warns (or the gantry hits a
  // finished part). See the extruder-clearance settings on the printer profile.
  print_sequence: {
    notice: (value) =>
      value === 'by_object'
        ? {
            tone: 'warning',
            title: 'Sequential printing depends on your printer',
            text:
              'Each object is printed to completion before the next begins, so the printhead ' +
              'must clear every finished part. A part taller than the gantry clearance, or two ' +
              'parts closer than the extruder-clearance radius, can collide. The slicer warns ' +
              'you before slicing, but those clearances describe your machine and have to be ' +
              'set for it first.',
            link: {
              text: 'Set extruder clearances in printer settings',
              routerLink: '/settings/printers',
            },
          }
        : null,
  },
};

/** Resolve the notice (if any) that applies to `field` at its current `value`. */
export function noticeForField(field: FieldDef, value: unknown): FieldNotice | null {
  return FIELD_EXCEPTIONS[field.key]?.notice?.(value) ?? null;
}
