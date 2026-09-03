import { Injectable, computed, signal } from '@angular/core';

/**
 * Which kind of thing a measurement snaps to.
 *
 * - `'point'` — dot-to-dot: the exact surface point under the cursor.
 * - `'face'` — face-to-face: the same click point, but the face's outward
 *   normal is captured too, which unlocks the perpendicular gap and the angle
 *   between the two faces.
 */
export type MeasureTool = 'point' | 'face';

/**
 * Which distance reading the details modal and the 3D overlay emphasise.
 *
 * The straight-line `'direct'` distance is one number, but two hand-picked
 * points rarely line up on the axis the user actually cares about — so a single
 * component (`'x' | 'y' | 'z'`) can be singled out to read the gap along just
 * that axis.
 */
export type MeasureAxis = 'direct' | 'x' | 'y' | 'z';

/** A picked measurement endpoint, resolved to world space by the viewer. */
export interface MeasurePoint {
  /** World-space position in millimetres. */
  readonly world: readonly [number, number, number];
  /**
   * Outward unit face normal, present only for a `'face'` pick. Drives the
   * perpendicular-gap and face-angle readouts.
   */
  readonly normal?: readonly [number, number, number];
  /** scene-engine id (string form) of the object the point sits on. */
  readonly objectId: string;
}

/** The two normals were within this many degrees of (anti)parallel. */
const PARALLEL_TOLERANCE_DEG = 3;

/**
 * A fully-resolved measurement between two picked points, plus every derived
 * reading the card and the details modal show. Pure geometry — no rounding or
 * formatting happens here, only in the views.
 */
export interface MeasureResult {
  readonly a: MeasurePoint;
  readonly b: MeasurePoint;
  /** Straight-line 3D distance (mm). */
  readonly distance: number;
  /** Signed component vector `b - a` (mm). */
  readonly vector: readonly [number, number, number];
  /** Absolute per-axis distances `|b - a|` (mm). */
  readonly delta: readonly [number, number, number];
  /** 2D distances projected onto each world plane (mm). */
  readonly planar: { readonly xy: number; readonly xz: number; readonly yz: number };
  /**
   * Angle between the two outward face normals in degrees (0–180), only when
   * both endpoints are faces. Two sides of a flat slab read ~180°, two coplanar
   * faces ~0°.
   */
  readonly faceAngleDeg?: number;
  /**
   * Perpendicular gap between the two faces (mm), only when they are (anti)
   * parallel within {@link PARALLEL_TOLERANCE_DEG}. Undefined for skew faces,
   * where a single perpendicular distance is not well defined.
   */
  readonly perpendicular?: number;
}

function sub(
  b: readonly [number, number, number],
  a: readonly [number, number, number],
): [number, number, number] {
  return [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
}

function dot(a: readonly [number, number, number], b: readonly [number, number, number]): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function faceReadouts(
  a: MeasurePoint,
  b: MeasurePoint,
  vector: readonly [number, number, number],
): { faceAngleDeg?: number; perpendicular?: number } {
  if (!a.normal || !b.normal) {
    return {};
  }
  const na = a.normal;
  const nb = b.normal;
  const cos = Math.min(1, Math.max(-1, dot(na, nb)));
  const faceAngleDeg = (Math.acos(cos) * 180) / Math.PI;
  // Parallel or anti-parallel faces have a single well-defined gap: project the
  // span onto either face's normal. Skew faces do not, so it is left undefined.
  const cosTol = Math.cos((PARALLEL_TOLERANCE_DEG * Math.PI) / 180);
  const parallel = Math.abs(cos) >= cosTol;
  const perpendicular = parallel ? Math.abs(dot(vector, na)) : undefined;
  return { faceAngleDeg, perpendicular };
}

function measure(a: MeasurePoint, b: MeasurePoint): MeasureResult {
  const vector = sub(b.world, a.world);
  const [dx, dy, dz] = vector;
  const delta: [number, number, number] = [Math.abs(dx), Math.abs(dy), Math.abs(dz)];
  const distance = Math.hypot(dx, dy, dz);
  const planar = {
    xy: Math.hypot(dx, dy),
    xz: Math.hypot(dx, dz),
    yz: Math.hypot(dy, dz),
  };
  return { a, b, distance, vector, delta, planar, ...faceReadouts(a, b, vector) };
}

/**
 * Single source of truth for the on-plate measuring tool.
 *
 * Holds only *state* — the active flag, the snap mode, the two picked
 * endpoints and which axis is emphasised — and derives every distance from it.
 * The viewer resolves clicks into world-space {@link MeasurePoint}s and feeds
 * them to {@link pick}; the toolbar card and the details modal read the derived
 * {@link result}. Deliberately root-provided so the tool, the card, the modal
 * and the context menu all see the same measurement.
 */
@Injectable({ providedIn: 'root' })
export class Measure {
  /** Whether the measuring tool is currently taking picks. */
  readonly active = signal(false);

  /** What a click snaps to. Switching it starts a fresh measurement. */
  readonly tool = signal<MeasureTool>('point');

  /** Which reading is emphasised in the overlay and the details modal. */
  readonly axis = signal<MeasureAxis>('direct');

  /** First picked endpoint, or `null` before the first click. */
  readonly pointA = signal<MeasurePoint | null>(null);
  /** Second picked endpoint, or `null` while only the first is placed. */
  readonly pointB = signal<MeasurePoint | null>(null);

  /** The resolved measurement once both endpoints are placed. */
  readonly result = computed<MeasureResult | null>(() => {
    const a = this.pointA();
    const b = this.pointB();
    return a && b ? measure(a, b) : null;
  });

  /** True once both endpoints are placed and a distance can be read. */
  readonly complete = computed(() => this.result() !== null);

  /** Turn the tool on (optionally choosing the snap mode). */
  activate(tool?: MeasureTool): void {
    if (tool) {
      this.tool.set(tool);
    }
    this.active.set(true);
  }

  /** Turn the tool off and forget the in-progress measurement. */
  deactivate(): void {
    this.active.set(false);
    this.reset();
  }

  /** Flip the tool on/off, seeding a snap mode when turning on. */
  toggle(tool?: MeasureTool): void {
    if (this.active()) {
      this.deactivate();
    } else {
      this.activate(tool);
    }
  }

  /** Choose the snap mode; changing it clears the current picks. */
  setTool(tool: MeasureTool): void {
    if (this.tool() === tool) {
      return;
    }
    this.tool.set(tool);
    this.reset();
  }

  /** Emphasise a particular distance reading. */
  setAxis(axis: MeasureAxis): void {
    this.axis.set(axis);
  }

  /**
   * Record a picked endpoint.
   *
   * The first pick (or any pick once a pair is complete) starts a new
   * measurement; the second completes it — the usual rubber-band behaviour of a
   * measuring tool, so a third click begins measuring afresh from that point.
   */
  pick(point: MeasurePoint): void {
    const a = this.pointA();
    const b = this.pointB();
    if (!a || b) {
      this.pointA.set(point);
      this.pointB.set(null);
    } else {
      this.pointB.set(point);
    }
  }

  /** Drop both endpoints but stay armed on the same snap mode. */
  reset(): void {
    this.pointA.set(null);
    this.pointB.set(null);
  }
}
