import { describe, expect, it } from 'vitest';
import { noticeForField } from './field-exceptions';
import type { FieldDef } from '../models/field-def';

/**
 * These notices are the app's only defence against a setting that silently does
 * nothing. The chamber case is the sharp one: without the printer's
 * `heated_chamber` capability the slicer emits no chamber command at all, and a
 * chamber that never heats is indistinguishable from one that does until the
 * part warps — so a missed warning here is a failed print.
 */

function field(key: string, type: FieldDef['type'] = 'number'): FieldDef {
  return { key, type } as FieldDef;
}

describe('chamber temperature without a heated chamber', () => {
  it('warns, and links to where the capability is enabled', () => {
    const notice = noticeForField(field('chamber_temp'), 50, { heated_chamber: false });

    expect(notice).not.toBeNull();
    expect(notice?.tone).toBe('warning');
    // The consequence must be stated outright, not hedged.
    expect(notice?.text).toContain('No chamber command will be emitted');
    expect(notice?.link?.routerLink).toBe('/settings/printers');
  });

  it('stays silent once the printer declares the capability', () => {
    expect(noticeForField(field('chamber_temp'), 50, { heated_chamber: true })).toBeNull();
  });

  it('stays silent when no chamber is asked for', () => {
    // Not wanting a chamber is not a misconfiguration; warning about it would
    // train users to ignore warnings.
    expect(noticeForField(field('chamber_temp'), 0, { heated_chamber: false })).toBeNull();
  });

  it('treats an absent capability as unknown rather than disabled', () => {
    // A printer profile saved before `heated_chamber` existed has not opted out,
    // and a notice the user cannot act on is just noise.
    expect(noticeForField(field('chamber_temp'), 50, {})).toBeNull();
  });

  it('applies to the first-layer soak target too', () => {
    const notice = noticeForField(field('chamber_temp_first_layer'), 60, {
      heated_chamber: false,
    });
    expect(notice?.link?.routerLink).toBe('/settings/printers');
  });
});

describe('fields without an exception', () => {
  it('produces no notice', () => {
    expect(noticeForField(field('layer_height'), 0.2, { heated_chamber: false })).toBeNull();
  });

  it('defaults siblings to empty, leaving cross-field conditions silent', () => {
    expect(noticeForField(field('chamber_temp'), 50)).toBeNull();
  });
});

describe('raft adhesion', () => {
  it('still warns that the preview will differ', () => {
    const notice = noticeForField(field('adhesion_type', 'string'), 'raft', {});
    expect(notice?.tone).toBe('warning');
  });

  it('says nothing for other adhesion types', () => {
    expect(noticeForField(field('adhesion_type', 'string'), 'brim', {})).toBeNull();
  });
});
