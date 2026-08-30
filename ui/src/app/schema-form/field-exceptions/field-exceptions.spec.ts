import { describe, expect, it } from 'vitest';
import { noticeForField } from './field-exceptions';
import type { FieldDef } from '../models/field-def';

/**
 * The field notices are the "transparent and honest" layer: a control whose
 * effect depends on something the user cannot see from here gets a caution and
 * a link to where that something lives. These pin the two that matter — a raft
 * changing the preview, and sequential printing depending on the printer — so a
 * refactor of the exceptions map cannot quietly drop them.
 */
function field(key: string): FieldDef {
  return { key, type: 'string', required: false };
}

describe('noticeForField', () => {
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

  it('warns that a raft shifts the preview', () => {
    const notice = noticeForField(field('adhesion_type'), 'raft');
    expect(notice?.tone).toBe('warning');
    expect(notice?.link).toBeUndefined();
  });

  it('returns null for a field with no exception', () => {
    expect(noticeForField(field('layer_height'), 0.2)).toBeNull();
  });
});
