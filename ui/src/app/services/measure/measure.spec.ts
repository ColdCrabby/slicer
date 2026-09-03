import { TestBed } from '@angular/core/testing';
import { beforeEach, describe, expect, it } from 'vitest';
import { Measure, type MeasurePoint } from './measure';

function setup() {
  TestBed.configureTestingModule({ providers: [Measure] });
  return TestBed.inject(Measure);
}

const pointAt = (
  world: [number, number, number],
  normal?: [number, number, number],
): MeasurePoint => ({ world, normal, objectId: '1' });

describe('Measure', () => {
  let measure: Measure;
  beforeEach(() => {
    measure = setup();
  });

  it('has no result until both endpoints are placed', () => {
    expect(measure.result()).toBeNull();
    measure.pick(pointAt([0, 0, 0]));
    expect(measure.pointA()).not.toBeNull();
    expect(measure.result()).toBeNull();
    measure.pick(pointAt([3, 4, 0]));
    expect(measure.result()).not.toBeNull();
  });

  it('computes the straight-line distance and per-axis deltas', () => {
    measure.pick(pointAt([1, 2, 3]));
    measure.pick(pointAt([4, 6, 15]));
    const r = measure.result()!;
    expect(r.delta).toEqual([3, 4, 12]);
    expect(r.distance).toBeCloseTo(13, 6); // 3-4-12 Pythagorean quadruple
    expect(r.vector).toEqual([3, 4, 12]);
  });

  it('reports absolute deltas regardless of pick order', () => {
    measure.pick(pointAt([10, 10, 10]));
    measure.pick(pointAt([4, 6, 2]));
    const r = measure.result()!;
    expect(r.delta).toEqual([6, 4, 8]);
    expect(r.vector).toEqual([-6, -4, -8]);
  });

  it('computes planar distances on each world plane', () => {
    measure.pick(pointAt([0, 0, 0]));
    measure.pick(pointAt([3, 4, 12]));
    const r = measure.result()!;
    expect(r.planar.xy).toBeCloseTo(5, 6);
    expect(r.planar.xz).toBeCloseTo(Math.hypot(3, 12), 6);
    expect(r.planar.yz).toBeCloseTo(Math.hypot(4, 12), 6);
  });

  it('rubber-bands: a third pick starts a fresh measurement', () => {
    measure.pick(pointAt([0, 0, 0]));
    measure.pick(pointAt([1, 0, 0]));
    expect(measure.complete()).toBe(true);
    measure.pick(pointAt([5, 5, 5]));
    expect(measure.pointA()!.world).toEqual([5, 5, 5]);
    expect(measure.pointB()).toBeNull();
    expect(measure.complete()).toBe(false);
  });

  it('gives parallel faces a perpendicular gap and a ~180° angle', () => {
    // Two sides of a 10mm slab: outward normals point opposite ways.
    measure.pick(pointAt([0, 0, 0], [0, 0, 1]));
    measure.pick(pointAt([3, 4, 10], [0, 0, -1]));
    const r = measure.result()!;
    expect(r.faceAngleDeg).toBeCloseTo(180, 4);
    expect(r.perpendicular).toBeCloseTo(10, 6); // only the Z span counts
  });

  it('leaves the perpendicular gap undefined for skew faces', () => {
    measure.pick(pointAt([0, 0, 0], [0, 0, 1]));
    measure.pick(pointAt([5, 0, 5], [1, 0, 0]));
    const r = measure.result()!;
    expect(r.faceAngleDeg).toBeCloseTo(90, 4);
    expect(r.perpendicular).toBeUndefined();
  });

  it('omits face readouts in point mode', () => {
    measure.pick(pointAt([0, 0, 0]));
    measure.pick(pointAt([1, 1, 1]));
    const r = measure.result()!;
    expect(r.faceAngleDeg).toBeUndefined();
    expect(r.perpendicular).toBeUndefined();
  });

  it('switching tool clears the in-progress picks', () => {
    measure.pick(pointAt([0, 0, 0]));
    measure.setTool('face');
    expect(measure.pointA()).toBeNull();
    // Same tool is a no-op and keeps picks.
    measure.pick(pointAt([1, 1, 1]));
    measure.setTool('face');
    expect(measure.pointA()).not.toBeNull();
  });

  it('deactivating forgets the measurement', () => {
    measure.activate('point');
    measure.pick(pointAt([0, 0, 0]));
    measure.pick(pointAt([1, 1, 1]));
    measure.deactivate();
    expect(measure.active()).toBe(false);
    expect(measure.result()).toBeNull();
  });
});
