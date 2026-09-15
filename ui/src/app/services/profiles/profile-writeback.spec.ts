import { TestBed } from '@angular/core/testing';
import { beforeEach, describe, expect, it } from 'vitest';
import { ProfileWriteback } from './profile-writeback';
import { ProfilePersistence, type ProfileCategory } from './profile-persistence';
import { PrintersStore } from './printers-store';
import { FilamentsStore } from './filaments-store';
import { PrintProfilesStore } from './print-profiles-store';
import { WorkplateSettingsStore } from '../workplate-settings';
import { WorkplatePersistence, type WorkplateSetup } from '../workplate-persistence';
import { SlicerFile } from '../slicer-file';

const PLATE = '11111111-1111-1111-1111-111111111111';

/** Local-only stand-in — no engine to write through to. */
class FakeProfilePersistence extends ProfilePersistence {
  readonly isEngineBacked = false;
  protected fetchLibrary() {
    return Promise.resolve({});
  }
  protected persistCategory(_category: ProfileCategory, _items: unknown[]) {
    return Promise.resolve();
  }
  exportLibrary(): never {
    throw new Error('not used in this test');
  }
}

class FakeWorkplatePersistence extends WorkplatePersistence {
  readonly isEngineBacked = false;
  async load(_requestUuid: string): Promise<WorkplateSetup | null> {
    return null;
  }
  async save(_requestUuid: string, _setup: WorkplateSetup): Promise<void> {}
}

function setUp(): ProfileWriteback {
  localStorage.clear();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: ProfilePersistence, useClass: FakeProfilePersistence },
      { provide: WorkplatePersistence, useClass: FakeWorkplatePersistence },
    ],
  });
  TestBed.inject(SlicerFile).requestUuid.set(PLATE);
  return TestBed.inject(ProfileWriteback);
}

describe('ProfileWriteback', () => {
  let writeback: ProfileWriteback;

  beforeEach(() => {
    writeback = setUp();
  });

  it('lists each override with its owning contract and preset value', () => {
    const workplateSettings = TestBed.inject(WorkplateSettingsStore);
    workplateSettings.setOverrides(PLATE, { layer_height: 0.12, nozzle_temp: 240, retract_mm: 2 });

    const rows = writeback.rows();
    const byKey = Object.fromEntries(rows.map((row) => [row.key, row]));

    expect(byKey['layer_height'].contract).toBe('process');
    expect(byKey['nozzle_temp'].contract).toBe('filament');
    expect(byKey['retract_mm'].contract).toBe('printer');
    expect(byKey['layer_height'].overrideValue).toBe(0.12);
  });

  it('starts every row accepted when the dialog opens', () => {
    const workplateSettings = TestBed.inject(WorkplateSettingsStore);
    workplateSettings.setOverrides(PLATE, { layer_height: 0.12, nozzle_temp: 240 });

    writeback.open();

    expect(writeback.isAccepted('layer_height')).toBe(true);
    expect(writeback.isAccepted('nozzle_temp')).toBe(true);
  });

  it('writes only accepted rows into their owning profile and clears them from overrides', () => {
    const workplateSettings = TestBed.inject(WorkplateSettingsStore);
    const profiles = TestBed.inject(PrintProfilesStore);
    const filaments = TestBed.inject(FilamentsStore);

    workplateSettings.setOverrides(PLATE, { layer_height: 0.12, nozzle_temp: 240 });
    const activeProfileId = profiles.items()[0]!.id;
    const activeFilamentId = filaments.items()[0]!.id;
    const originalNozzleTemp = filaments.getById(activeFilamentId)!.params?.nozzle_temp;

    writeback.open();
    writeback.setAccepted('nozzle_temp', false);
    writeback.apply();

    expect(profiles.getById(activeProfileId)!.params!.layer_height).toBe(0.12);
    expect(filaments.getById(activeFilamentId)!.params?.nozzle_temp).toBe(originalNozzleTemp);
    expect(workplateSettings.settingsFor(PLATE).overrides).toEqual({ nozzle_temp: 240 });
  });

  it('leaves the printer profile untouched when no printer setting is accepted', () => {
    const workplateSettings = TestBed.inject(WorkplateSettingsStore);
    const printers = TestBed.inject(PrintersStore);
    const activePrinter = printers.items()[0]!;
    const before = { ...activePrinter.params };

    workplateSettings.setOverrides(PLATE, { layer_height: 0.12 });
    writeback.open();
    writeback.apply();

    expect(printers.getById(activePrinter.id)!.params).toEqual(before);
  });
});
