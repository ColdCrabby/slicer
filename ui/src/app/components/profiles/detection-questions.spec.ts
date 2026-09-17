import { describe, expect, it } from 'vitest';
import { applyFanAnswer } from './detection-questions';

/** The part-cooling fan every cooling answer carries along. */
const PART_COOLING = {
  fan_index: 0,
  min_speed: 0.35,
  max_speed: 1.0,
  layer_time_fast_s: 10.0,
  layer_time_slow_s: 30.0,
};

/** What an answer contributes for a named auxiliary fan. */
function auxAnswer(fan: string) {
  return {
    fan_configs: [
      PART_COOLING,
      {
        fan_index: 3,
        klipper_name: fan,
        min_speed: 0.0,
        max_speed: 1.0,
        layer_time_fast_s: 10.0,
        layer_time_slow_s: 30.0,
        aux_overrides: { bridge_boost: 0.4 },
      },
    ],
  };
}

function fanNames(params: Record<string, unknown>): (string | null | undefined)[] {
  return (
    (params['fan_configs'] as { klipper_name?: string | null }[] | undefined)?.map(
      (entry) => entry.klipper_name ?? null,
    ) ?? []
  );
}

describe('applyFanAnswer', () => {
  it('keeps both fans when a machine asks about two of them', () => {
    // The regression this function exists for: a plain key merge let the second
    // answer's whole array replace the first's, so only the last fan survived.
    const first = applyFanAnswer({}, 'rscs', auxAnswer('rscs'));
    const both = applyFanAnswer(first, 'exhaust', auxAnswer('exhaust'));

    expect(fanNames(both)).toEqual([null, 'rscs', 'exhaust']);
  });

  it('carries exactly one part-cooling fan however many answers arrive', () => {
    // Every cooling answer ships P0 so no profile ends up without one; the
    // array must not accumulate a copy per answer.
    const both = applyFanAnswer(
      applyFanAnswer({}, 'rscs', auxAnswer('rscs')),
      'exhaust',
      auxAnswer('exhaust'),
    );
    const partCooling = (both['fan_configs'] as { fan_index: number }[]).filter(
      (entry) => entry.fan_index === 0,
    );

    expect(partCooling).toHaveLength(1);
  });

  it('retracts a fan when the answer is changed back to unused', () => {
    const configured = applyFanAnswer({}, 'rscs', auxAnswer('rscs'));
    const retracted = applyFanAnswer(configured, 'rscs', {});

    expect(fanNames(retracted).includes('rscs')).toBe(false);
  });

  it('drops the key entirely when the last answer is retracted', () => {
    // An empty array is not the same as an absent one: the engine's serde
    // default only fills a field that is missing, so leaving `[]` behind would
    // mean a printer with no part-cooling fan at all.
    const configured = applyFanAnswer({}, 'rscs', {
      fan_configs: [{ fan_index: 3, klipper_name: 'rscs' }],
    });
    const retracted = applyFanAnswer(configured, 'rscs', {});

    expect('fan_configs' in retracted).toBe(false);
  });

  it('leaves another fan alone when one of them is retracted', () => {
    const both = applyFanAnswer(
      applyFanAnswer({}, 'rscs', auxAnswer('rscs')),
      'exhaust',
      auxAnswer('exhaust'),
    );
    const retracted = applyFanAnswer(both, 'rscs', {});

    expect(fanNames(retracted)).toEqual([null, 'exhaust']);
  });

  it('replaces this fan when the answer changes from one role to another', () => {
    const full = applyFanAnswer({}, 'rscs', auxAnswer('rscs'));
    const gentle = applyFanAnswer(full, 'rscs', {
      fan_configs: [
        PART_COOLING,
        { fan_index: 3, klipper_name: 'rscs', aux_overrides: { speed_scale: 0.6 } },
      ],
    });

    const rscs = (
      gentle['fan_configs'] as { klipper_name?: string; aux_overrides?: unknown }[]
    ).filter((entry) => entry.klipper_name === 'rscs');
    expect(rscs).toHaveLength(1);
    expect(rscs[0].aux_overrides).toEqual({ speed_scale: 0.6 });
  });

  it('passes every other key of the answer straight through', () => {
    const merged = applyFanAnswer({ nozzle_diameter_mm: 0.4 }, 'rscs', {
      fan_configs: [PART_COOLING],
      heated_chamber: true,
    });

    expect(merged['nozzle_diameter_mm']).toBe(0.4);
    expect(merged['heated_chamber']).toBe(true);
  });

  it('does not modify the bag it was given', () => {
    const params = { fan_configs: [PART_COOLING] };
    applyFanAnswer(params, 'rscs', auxAnswer('rscs'));

    expect(params.fan_configs).toHaveLength(1);
  });
});
