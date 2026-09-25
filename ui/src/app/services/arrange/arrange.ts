import { Injectable, Injector, computed, inject, signal } from '@angular/core';
import { BrowserStorage } from '../browser-storage';
import { ActiveSelection } from '../profiles/active-selection';
import { SceneCommand } from '../scene-command/scene-command';
import { SceneEngine } from '../scene-engine';
import { Slicer } from '../slicer';

const SPACING_KEY = 'nexus.viewer.arrangeSpacingMm';
const AUTO_ORIENT_KEY = 'nexus.viewer.arrangeAutoOrient';
const TURN_TO_FIT_KEY = 'nexus.viewer.arrangeTurnToFit';

/**
 * Quarter turn the packer may give a part to make it fit, in degrees.
 *
 * A quarter turn is the one rotation that costs nothing: it cannot undo an
 * auto-orient result, and it keeps a printer's preferred 45° diagonal.
 */
const TURN_STEP_DEG = 90;

/** Gap left between objects when placing them, in millimetres. */
export const DEFAULT_ARRANGE_SPACING_MM = 4;
export const MIN_ARRANGE_SPACING_MM = 0;
export const MAX_ARRANGE_SPACING_MM = 50;

/** Everything the engine needs to lay parts out, resolved from prefs + printer. */
export interface ArrangeSettings {
  spacingMm: number;
  autoOrient: boolean;
  /** Rotation step the packer may use, in degrees. `0` keeps every angle. */
  rotationStepDeg: number;
  /** May a part take plate area above or below another part? */
  verticalNesting: boolean;
  /** Underside angle past which a part is taken to need support (degrees). */
  overhangThresholdDeg: number;
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
  private readonly injector = inject(Injector);

  /**
   * The slicer, resolved on first read rather than at construction.
   *
   * `Slicer` injects `WorkplateObjects`, which injects this service; taking
   * `Slicer` eagerly closes that loop and Angular refuses to build any of the
   * three (NG0200), which leaves the whole app blank. Only the two computeds
   * below read it, and they run long after construction has finished.
   */
  private get slicer(): Slicer {
    return this.injector.get(Slicer);
  }

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
   * Whether the packer may give a part a quarter turn to make it fit.
   *
   * On by default: parts are nested by their real outline, and being allowed
   * to turn one is most of what lets an awkward plate close up. Off keeps
   * every part on the angle it is on, for a plate laid out by hand.
   */
  readonly turnToFit = signal<boolean>(this.readTurnToFit());

  /**
   * Extra Z-rotation the active printer prefers, in degrees. `0` when the
   * machine has no preference.
   */
  readonly preferredOrientationDeg = computed(
    () => this.activeSelection.printer()?.preferred_orientation_deg ?? 0,
  );

  /**
   * Whether parts may share plate area at different heights.
   *
   * True while the plate rises a layer at a time: the nozzle is always at the
   * height of the tallest thing printed so far, so a part leaning at 45° may
   * hang over its neighbour. Printing one part at a time drives the gantry
   * past finished parts instead, and then every part needs its own column.
   */
  readonly verticalNesting = computed(() => this.slicer.settings().print_sequence !== 'by_object');

  /**
   * The support threshold the plate will actually be sliced with.
   *
   * The packer frees the space under an underside steep enough to print over
   * thin air; taking the angle from the process that will slice the plate is
   * what keeps that promise true.
   */
  readonly overhangThresholdDeg = computed(
    () => this.slicer.settings().support_threshold_angle ?? 45,
  );

  /** How many objects a "place all" would move. */
  readonly objectCount = computed(() => this.sceneEngine.objects().length);

  /** The resolved settings a run would use. */
  readonly settings = computed<ArrangeSettings>(() => ({
    spacingMm: this.spacingMm(),
    autoOrient: this.autoOrient(),
    rotationStepDeg: this.turnToFit() ? TURN_STEP_DEG : 0,
    verticalNesting: this.verticalNesting(),
    overhangThresholdDeg: this.overhangThresholdDeg(),
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

  /** Set whether the packer may turn a part to make it fit, and persist it. */
  setTurnToFit(value: boolean): void {
    this.turnToFit.set(value);
    this.storage.write(TURN_TO_FIT_KEY, String(value));
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
          rotation_step_deg: settings.rotationStepDeg,
          vertical_nesting: settings.verticalNesting,
          orient_options: {
            preferred_z_rotation_deg: settings.preferredOrientationDeg,
            overhang_threshold_deg: settings.overhangThresholdDeg,
          },
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

  private readTurnToFit(): boolean {
    // Unset means "on": the packer has always been free to place a part
    // wherever it fits, and a quarter turn is part of that.
    return this.storage.get(TURN_TO_FIT_KEY)() !== 'false';
  }

  private readAutoOrient(): boolean {
    // Unset means "on": dropping a file in has always oriented it, and the
    // toggle now governs that same placement.
    return this.storage.get(AUTO_ORIENT_KEY)() !== 'false';
  }
}
