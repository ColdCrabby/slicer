import { computed, inject, Injectable } from '@angular/core';
import {
  displayUnitOf,
  familyOptions,
  offersUnit,
  switchableFamilies,
  type UnitDisplay,
} from '../schema-form/models/field-units';
import { BrowserStorage } from './browser-storage';

const UNIT_DISPLAY_KEY = 'general.unitDisplay';

/**
 * Which unit each convertible family is *shown* in.
 *
 * The engine's storage units are not up for debate — travel and retraction
 * speed go on an `F` word and that word is mm/min — but reading them is. Almost
 * everyone in 3D printing thinks in mm/s, so a panel that asks for `9000`
 * beside a print speed of `120` is asking the user to do arithmetic to notice
 * the two are the same kind of number. The default here is mm/s for every
 * speed, and the stored value is converted on the way in and out.
 *
 * The preference is **per family, not per field**, and that is the whole point:
 * a plate where travel reads in mm/min and print speed in mm/s is worse than
 * either unit chosen consistently. Pressing the unit on any one speed field
 * switches every speed field in the app.
 *
 * It is a display preference of this browser, so it lives in `localStorage`
 * beside the other view preferences rather than in a profile — nothing about
 * the sliced result changes, and a profile shared with someone else should not
 * carry how its author likes to read it.
 */
@Injectable({ providedIn: 'root' })
export class UnitPreference {
  private readonly storage = inject(BrowserStorage);
  private readonly stored = this.storage.get(UNIT_DISPLAY_KEY, 'local');

  /**
   * The chosen display unit per family. Anything the stored JSON cannot supply
   * — absent, malformed, or naming a unit the family no longer offers — is left
   * out, and `unitForField` falls back to the family's own default.
   */
  readonly display = computed<UnitDisplay>(() => {
    const raw = this.stored();
    if (!raw) {
      return {};
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return {};
    }
    if (typeof parsed !== 'object' || parsed === null) {
      return {};
    }
    const record = parsed as Record<string, unknown>;
    const chosen: Record<string, string> = {};
    for (const family of switchableFamilies()) {
      const value = record[family];
      if (typeof value === 'string' && offersUnit(family, value)) {
        chosen[family] = value;
      }
    }
    return chosen;
  });

  /** Show `family` in the unit with this id. Unknown ids are ignored. */
  set(family: string, unitId: string): void {
    if (!offersUnit(family, unitId)) {
      return;
    }
    this.storage.writeJson(UNIT_DISPLAY_KEY, { ...this.display(), [family]: unitId }, 'local');
  }

  /** Advance to the family's next display unit, wrapping at the end. */
  cycle(family: string): void {
    const options = familyOptions(family);
    if (options.length < 2) {
      return;
    }
    // Resolve through the family default, or the first press of a never-touched
    // toggle would "advance" to the unit already on screen.
    const shown = displayUnitOf(family, this.display());
    const current = options.findIndex((o) => o.id === shown);
    this.set(family, options[(current + 1) % options.length].id);
  }
}
