import { describe, expect, it } from 'vitest';
import globalSettingsSchema from '../../schemas/slicer-engine-global-settings-v1.json';
import { parseSchema } from '../schema-form/models/schema-parser';
import {
  GROUP_ICONS,
  SETTING_CONTRACTS,
  bucketGroupsByContract,
  contractForGroup,
} from './setting-contract';

/**
 * The settings sidebar is schema-driven, so a new engine parameter appears
 * without anyone editing the UI — but the *group* it belongs to still has to be
 * claimed by a contract and given an icon. An unclaimed group is not broken,
 * just quietly wrong: it falls to the end of Process, out of taxonomy order,
 * with a blank where its icon should be.
 *
 * These tests read the real engine schema, so adding an `x-group` in Rust and
 * forgetting the two lines here fails the build rather than shipping.
 */
describe('setting contracts', () => {
  const schema = globalSettingsSchema as unknown as {
    $defs: Record<string, Record<string, unknown>>;
  };
  const groups = parseSchema({ ...schema.$defs['SlicingParams'], $defs: schema.$defs }).groups.map(
    (g) => g.name,
  );

  it('claims every group the engine declares', () => {
    const claimed = new Set(SETTING_CONTRACTS.flatMap((c) => c.groups));
    const unclaimed = groups.filter((g) => !claimed.has(g));
    expect(unclaimed, `unclaimed x-group(s): ${unclaimed.join(', ')}`).toEqual([]);
  });

  it('gives every group an icon', () => {
    const iconless = groups.filter((g) => !GROUP_ICONS[g]);
    expect(iconless, `x-group(s) without an icon: ${iconless.join(', ')}`).toEqual([]);
  });

  it('files dimensional compensation under Process, beside the other print settings', () => {
    expect(groups).toContain('Dimensions');
    expect(contractForGroup('Dimensions')).toBe('process');
    expect(bucketGroupsByContract(groups).process).toContain('Dimensions');
  });
});
