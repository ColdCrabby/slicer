import { TestBed } from '@angular/core/testing';
import { signal } from '@angular/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { BrowserStorage } from '../browser-storage';
import { ActiveSelection } from '../profiles/active-selection';
import { SceneCommand } from '../scene-command/scene-command';
import { SceneEngine, type SceneOp } from '../scene-engine';
import { Arrange } from './arrange';

/** Minimal stand-ins: `Arrange` touches only a sliver of each collaborator. */
function setup(options: { stored?: Record<string, string>; preferredDeg?: number } = {}) {
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

  TestBed.configureTestingModule({
    providers: [
      { provide: BrowserStorage, useValue: storage },
      { provide: SceneCommand, useValue: sceneCommand },
      { provide: SceneEngine, useValue: sceneEngine },
      { provide: ActiveSelection, useValue: activeSelection },
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
          orient_options: { preferred_z_rotation_deg: 45 },
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
