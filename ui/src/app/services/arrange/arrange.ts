import { Injectable, computed, inject, signal } from '@angular/core';
import { BrowserStorage } from '../browser-storage';
import { ActiveSelection } from '../profiles/active-selection';
import { SceneCommand } from '../scene-command/scene-command';
import { SceneEngine } from '../scene-engine';

const SPACING_KEY = 'nexus.viewer.arrangeSpacingMm';
const AUTO_ORIENT_KEY = 'nexus.viewer.arrangeAutoOrient';

/** Gap left between objects when placing them, in millimetres. */
export const DEFAULT_ARRANGE_SPACING_MM = 4;
export const MIN_ARRANGE_SPACING_MM = 0;
export const MAX_ARRANGE_SPACING_MM = 50;

/** Everything the engine needs to lay parts out, resolved from prefs + printer. */
export interface ArrangeSettings {
  spacingMm: number;
  autoOrient: boolean;
  /** Extra Z-rotation applied after auto-orient, from the active printer. */
  preferredOrientationDeg: number;
}

/**
 * The one way objects get placed on the plate.
 *
 * "Auto-orient" and "arrange all objects" used to be two rival commands:
 * orienting left parts overlapping, and arranging could not fix a part lying
 * on a bad face. They are one operation here — *place the objects* — with
 * auto-orient as a setting of that operation rather than a competing button.
 * The engine already models it that way (`ArrangeOnBed` takes an `auto_orient`
 * flag and forwards `orient_options`), so this service is simply the UI half
 * of a contract that already existed.
 *
 * The preferred Z-rotation comes from the **active printer**, not from these
 * preferences: printing everything at 45° is a property of the machine (CoreXY
 * moves fastest along its diagonals), so it follows the printer the user picks
 * instead of being re-set per plate.
 */
@Injectable({ providedIn: 'root' })
export class Arrange {
  private readonly storage = inject(BrowserStorage);
  private readonly sceneCommand = inject(SceneCommand);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly activeSelection = inject(ActiveSelection);

  /**
   * Gap left between objects (mm).
   *
   * Spacing is a real print concern — parts need clearance for the nozzle and
   * for a brim — so it is a user setting rather than a constant, and it
   * persists across sessions.
   */
  readonly spacingMm = signal<number>(this.readSpacing());

  /**
   * Whether placing also re-orients each object to minimise overhangs.
   *
   * On by default — the same as dropping a file in, which has always landed
   * the model on its flattest face. Turning it off keeps every pose exactly as
   * the user (or the file) left it and only moves parts apart.
   */
  readonly autoOrient = signal<boolean>(this.readAutoOrient());

  /**
   * Extra Z-rotation the active printer prefers, in degrees. `0` when the
   * machine has no preference.
   */
  readonly preferredOrientationDeg = computed(
    () => this.activeSelection.printer()?.preferred_orientation_deg ?? 0,
  );

  /** How many objects a "place all" would move. */
  readonly objectCount = computed(() => this.sceneEngine.objects().length);

  /**
   * Whether the contextual placement card is showing.
   *
   * Lives here rather than in the toolbar button because the button and the
   * card are separate components docked in different parts of the shell —
   * exactly like the object-mode buttons and the transform card they open.
   */
  readonly optionsOpen = signal(false);

  /** Toggle the contextual placement card. */
  toggleOptions(): void {
    this.optionsOpen.update((open) => !open);
  }

  /** Close the contextual placement card. */
  closeOptions(): void {
    this.optionsOpen.set(false);
  }

  /** The resolved settings a run would use. */
  readonly settings = computed<ArrangeSettings>(() => ({
    spacingMm: this.spacingMm(),
    autoOrient: this.autoOrient(),
    preferredOrientationDeg: this.preferredOrientationDeg(),
  }));

  /** Set the gap between objects (mm), clamped to a sane range, and persist it. */
  setSpacingMm(value: number): void {
    const clamped = Number.isFinite(value)
      ? Math.max(MIN_ARRANGE_SPACING_MM, Math.min(MAX_ARRANGE_SPACING_MM, value))
      : DEFAULT_ARRANGE_SPACING_MM;
    this.spacingMm.set(clamped);
    this.storage.write(SPACING_KEY, String(clamped));
  }

  /** Set whether placing re-orients each part, and persist it. */
  setAutoOrient(value: boolean): void {
    this.autoOrient.set(value);
    this.storage.write(AUTO_ORIENT_KEY, String(value));
  }

  /**
   * Place objects on the plate: optionally auto-orient each one, then pack
   * them without overlap and centre the result on the bed.
   *
   * `ids` narrows the run to a selection; omitted or empty places everything.
   * A single object still goes through `ArrangeOnBed` — packing one part is
   * exactly "orient it and centre it", which is what the user expects from
   * the same button.
   */
  run(ids?: readonly bigint[]): void {
    const targets =
      ids && ids.length > 0 ? [...ids] : this.sceneEngine.objects().map((object) => object.id);
    if (targets.length === 0) {
      return;
    }

    const settings = this.settings();
    this.sceneCommand.apply({
      op: 'ArrangeOnBed',
      args: {
        ids: targets,
        options: {
          spacing_mm: settings.spacingMm,
          auto_orient: settings.autoOrient,
          orient_options: { preferred_z_rotation_deg: settings.preferredOrientationDeg },
        },
      },
    });
    this.sceneCommand.flush();
  }

  private readSpacing(): number {
    // `Number(null)` is 0, not NaN — checking finiteness alone would silently
    // turn "never set" into a 0 mm gap and place parts touching.
    const raw = this.storage.get(SPACING_KEY)();
    if (raw === null || raw.trim() === '') {
      return DEFAULT_ARRANGE_SPACING_MM;
    }
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) {
      return DEFAULT_ARRANGE_SPACING_MM;
    }
    return Math.max(MIN_ARRANGE_SPACING_MM, Math.min(MAX_ARRANGE_SPACING_MM, parsed));
  }

  private readAutoOrient(): boolean {
    // Unset means "on": dropping a file in has always oriented it, and the
    // toggle now governs that same placement.
    return this.storage.get(AUTO_ORIENT_KEY)() !== 'false';
  }
}
