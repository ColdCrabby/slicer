import { describe, expect, it } from 'vitest';
import {
  describeMeshDefects,
  describeMeshRepairs,
  meshHasRemainingDefects,
  meshReportIsNoteworthy,
  meshWasClean,
  type MeshDiagnostics,
  type MeshReport,
  type MeshRepairActions,
} from './mesh-report.model';

const CLEAN: MeshDiagnostics = {
  triangles: 12,
  vertices: 8,
  shells: 1,
  degenerate_faces: 0,
  duplicate_faces: 0,
  non_manifold_edges: 0,
  boundary_edges: 0,
  holes: 0,
  largest_hole_edges: 0,
  inconsistent_winding_edges: 0,
  inverted_shells: 0,
};

const NO_ACTIONS: MeshRepairActions = {
  welded_vertices: 0,
  removed_degenerate_faces: 0,
  removed_duplicate_faces: 0,
  flipped_faces: 0,
  filled_holes: 0,
  added_fill_triangles: 0,
  unfilled_holes: 0,
};

function report(partial: Partial<MeshReport>): MeshReport {
  return {
    before: CLEAN,
    after: CLEAN,
    actions: NO_ACTIONS,
    repaired: false,
    summary: '',
    ...partial,
  };
}

describe('mesh report', () => {
  it('says nothing about a clean model', () => {
    const r = report({});
    expect(meshWasClean(r)).toBe(true);
    expect(meshHasRemainingDefects(r)).toBe(false);
    expect(meshReportIsNoteworthy(r)).toBe(false);
  });

  it('is noteworthy when a defect was repaired', () => {
    const r = report({
      before: { ...CLEAN, holes: 1, boundary_edges: 4 },
      actions: { ...NO_ACTIONS, filled_holes: 1, added_fill_triangles: 4 },
      repaired: true,
    });
    expect(meshWasClean(r)).toBe(false);
    expect(meshHasRemainingDefects(r)).toBe(false);
    expect(meshReportIsNoteworthy(r)).toBe(true);
  });

  it('is noteworthy when a defect survives the repair pass', () => {
    const r = report({
      before: { ...CLEAN, non_manifold_edges: 2 },
      after: { ...CLEAN, non_manifold_edges: 2 },
    });
    expect(meshHasRemainingDefects(r)).toBe(true);
    expect(meshReportIsNoteworthy(r)).toBe(true);
  });

  it('counts an inside-out but otherwise sound mesh as defective', () => {
    const r = report({ before: { ...CLEAN, inverted_shells: 1 } });
    expect(meshWasClean(r)).toBe(false);
  });

  it('describes repairs in singular and plural', () => {
    expect(describeMeshRepairs({ ...NO_ACTIONS, filled_holes: 1 })).toBe('filled 1 hole');
    expect(describeMeshRepairs({ ...NO_ACTIONS, filled_holes: 3, flipped_faces: 2 })).toBe(
      'filled 3 holes, flipped 2 triangles',
    );
    expect(describeMeshRepairs(NO_ACTIONS)).toBeNull();
  });

  it('describes remaining defects, worst first', () => {
    expect(describeMeshDefects({ ...CLEAN, non_manifold_edges: 1, holes: 2 })).toBe(
      '1 non-manifold edge, 2 unfilled holes',
    );
    expect(describeMeshDefects(CLEAN)).toBeNull();
  });
});
