import { describe, expect, it } from 'vitest';
import { noticeForField } from './field-exceptions';
import type { FieldDef, FieldType } from '../models/field-def';

/**
 * The field notices are the "transparent and honest" layer: a control whose
 * effect depends on something the user cannot see from here gets a caution and
 * a link to where that something lives. These pin the three that matter — a
 * raft changing the preview, sequential printing depending on the printer's
 * clearances, and a chamber temperature depending on the printer having a
 * chamber heater — so a refactor of the exceptions map cannot quietly drop
 * them.
 *
 * The chamber case is the sharpest: without `heated_chamber` the slicer emits
 * no chamber command at all, and a chamber that never heats is
 * indistinguishable from one that does until the part warps. A missed warning
 * there is a failed print.
 */
function field(key: string, type: FieldType = 'string'): FieldDef {
  return { key, type, required: false };
}

describe('sequential printing', () => {
  it('warns and links to printer settings when sequential printing is chosen', () => {
    const notice = noticeForField(field('print_sequence'), 'by_object');
    expect(notice).not.toBeNull();
    expect(notice?.tone).toBe('warning');
    expect(notice?.link?.routerLink).toBe('/settings/printers');
    // The message must name the actual failure mode, not just say "be careful".
    expect(notice?.text).toMatch(/clearance/i);
  });

  it('stays silent for layer-by-layer printing', () => {
    expect(noticeForField(field('print_sequence'), 'by_layer')).toBeNull();
  });
});

describe('chamber temperature without a heated chamber', () => {
  it('warns, and links to where the capability is enabled', () => {
    const notice = noticeForField(field('chamber_temp', 'number'), 50, {
      heated_chamber: false,
    });

    expect(notice).not.toBeNull();
    expect(notice?.tone).toBe('warning');
    // The consequence must be stated outright, not hedged.
    expect(notice?.text).toContain('No chamber command will be emitted');
    expect(notice?.link?.routerLink).toBe('/settings/printers');
  });

  it('stays silent once the printer declares the capability', () => {
    expect(
      noticeForField(field('chamber_temp', 'number'), 50, { heated_chamber: true }),
    ).toBeNull();
  });

  it('stays silent when no chamber is asked for', () => {
    // Not wanting a chamber is not a misconfiguration; warning about it would
    // train users to ignore warnings.
    expect(
      noticeForField(field('chamber_temp', 'number'), 0, { heated_chamber: false }),
    ).toBeNull();
  });

  it('treats an absent capability as unknown rather than disabled', () => {
    // A printer profile saved before `heated_chamber` existed has not opted out,
    // and a notice the user cannot act on is just noise.
    expect(noticeForField(field('chamber_temp', 'number'), 50, {})).toBeNull();
  });

  it('applies to the first-layer soak target too', () => {
    const notice = noticeForField(field('chamber_temp_first_layer', 'number'), 60, {
      heated_chamber: false,
    });
    expect(notice?.link?.routerLink).toBe('/settings/printers');
  });
});

describe('raft adhesion', () => {
  it('warns that a raft shifts the preview', () => {
    const notice = noticeForField(field('adhesion_type'), 'raft');
    expect(notice?.tone).toBe('warning');
    expect(notice?.link).toBeUndefined();
  });

  it('says nothing for other adhesion types', () => {
    expect(noticeForField(field('adhesion_type'), 'brim')).toBeNull();
  });
});

describe('fields without an exception', () => {
  it('returns null for a field with no exception', () => {
    expect(noticeForField(field('layer_height', 'number'), 0.2)).toBeNull();
  });

  it('defaults siblings to empty, leaving cross-field conditions silent', () => {
    expect(noticeForField(field('chamber_temp', 'number'), 50)).toBeNull();
  });
});
