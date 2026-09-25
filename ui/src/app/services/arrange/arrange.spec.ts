import { TestBed } from '@angular/core/testing';
import { signal } from '@angular/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { BrowserStorage } from '../browser-storage';
import { ActiveSelection } from '../profiles/active-selection';
import { SceneCommand } from '../scene-command/scene-command';
import { SceneEngine, type SceneOp } from '../scene-engine';
import { Slicer } from '../slicer';
import { Arrange } from './arrange';

/** Minimal stand-ins: `Arrange` touches only a sliver of each collaborator. */
function setup(
  options: {
    stored?: Record<string, string>;
    preferredDeg?: number;
    printSequence?: string;
    supportThresholdDeg?: number;
  } = {},
) {
  const stored = new Map(Object.entries(options.stored ?? {}));
  const applied: SceneOp[] = [];

  const storage = {
    get: (key: string) => signal(stored.get(key) ?? null),
    write: (key: string, value: string | null) => {
      value === null ? stored.delete(key) : stored.set(key, value);
    },
  };

  const sceneCommand = {
    apply: (op: SceneOp) => applied.push(op),
    flush: vi.fn(),
  };

  const sceneEngine = {
    objects: signal([{ id: 1n }, { id: 2n }]),
  };

  const activeSelection = {
    printer: signal({ id: 'p1', name: 'Voron', preferred_orientation_deg: options.preferredDeg }),
  };

  // The packer asks the plate's own settings two questions: whether the plate
  // rises together, and which undersides hold themselves up.
  const slicer = {
    settings: signal({
      print_sequence: options.printSequence ?? 'by_layer',
      support_threshold_angle: options.supportThresholdDeg ?? 45,
    }),
  };

  TestBed.configureTestingModule({
    providers: [
      { provide: BrowserStorage, useValue: storage },
      { provide: SceneCommand, useValue: sceneCommand },
      { provide: SceneEngine, useValue: sceneEngine },
      { provide: ActiveSelection, useValue: activeSelection },
      { provide: Slicer, useValue: slicer },
    ],
  });

  return { arrange: TestBed.inject(Arrange), applied, stored, sceneEngine };
}

describe('Arrange', () => {
  beforeEach(() => TestBed.resetTestingModule());

  it('auto-orients by default so placing matches how a dropped file lands', () => {
    expect(setup().arrange.autoOrient()).toBe(true);
  });

  it('remembers an explicit opt-out', () => {
    const { arrange } = setup({ stored: { 'nexus.viewer.arrangeAutoOrient': 'false' } });
    expect(arrange.autoOrient()).toBe(false);
  });

  it('clamps the gap to the supported range and persists it', () => {
    const { arrange, stored } = setup();
    arrange.setSpacingMm(999);
    expect(arrange.spacingMm()).toBe(50);
    expect(stored.get('nexus.viewer.arrangeSpacingMm')).toBe('50');
  });

  it('falls back to the default gap when none was ever stored', () => {
    // `Number(null)` is 0, not NaN, so a finiteness check alone would read
    // "never set" as a 0 mm gap and place parts touching.
    expect(setup().arrange.spacingMm()).toBe(4);
  });

  it('honours a stored gap of zero', () => {
    const { arrange } = setup({ stored: { 'nexus.viewer.arrangeSpacingMm': '0' } });
    expect(arrange.spacingMm()).toBe(0);
  });

  it('lets the packer turn parts by default', () => {
    expect(setup().arrange.turnToFit()).toBe(true);
  });

  it('remembers a plate laid out by hand, where turning is off', () => {
    const { arrange } = setup({ stored: { 'nexus.viewer.arrangeTurnToFit': 'false' } });
    expect(arrange.turnToFit()).toBe(false);
  });

  it('sends no rotation step once turning is off, so hand-set angles survive', () => {
    const { arrange, applied, stored } = setup();
    arrange.setTurnToFit(false);
    arrange.run();
    expect(stored.get('nexus.viewer.arrangeTurnToFit')).toBe('false');
    expect(applied[0]).toMatchObject({ args: { options: { rotation_step_deg: 0 } } });
  });

  it('sends one ArrangeOnBed carrying gap, auto-orient and the printer angle', () => {
    // The whole point of merging the two commands: a single op decides both
    // orientation and layout, so they cannot disagree.
    const { arrange, applied } = setup({ preferredDeg: 45 });
    arrange.setSpacingMm(6);
    arrange.run();

    expect(applied).toHaveLength(1);
    expect(applied[0]).toEqual({
      op: 'ArrangeOnBed',
      args: {
        ids: [1n, 2n],
        options: {
          spacing_mm: 6,
          auto_orient: true,
          rotation_step_deg: 90,
          vertical_nesting: true,
          orient_options: { preferred_z_rotation_deg: 45, overhang_threshold_deg: 45 },
        },
      },
    });
  });

  it('treats a printer with no preference as no extra rotation', () => {
    const { arrange, applied } = setup();
    arrange.run();
    expect(applied[0]).toMatchObject({
      args: { options: { orient_options: { preferred_z_rotation_deg: 0 } } },
    });
  });

  it('gives every part its own column when the plate prints one at a time', () => {
    // The gantry drives past finished parts, so nothing may pass over anything.
    const { arrange, applied } = setup({ printSequence: 'by_object' });
    arrange.run();
    expect(applied[0]).toMatchObject({ args: { options: { vertical_nesting: false } } });
  });

  it('packs against the support threshold the plate will be sliced with', () => {
    const { arrange, applied } = setup({ supportThresholdDeg: 60 });
    arrange.run();
    expect(applied[0]).toMatchObject({
      args: { options: { orient_options: { overhang_threshold_deg: 60 } } },
    });
  });

  it('narrows to a selection when given ids', () => {
    const { arrange, applied } = setup();
    arrange.run([2n]);
    expect(applied[0]).toMatchObject({ args: { ids: [2n] } });
  });

  it('does nothing on an empty plate', () => {
    const { arrange, applied, sceneEngine } = setup();
    sceneEngine.objects.set([]);
    arrange.run();
    expect(applied).toHaveLength(0);
  });
});
