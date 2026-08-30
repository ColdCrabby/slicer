/**
 * Mirror of the Rust `crate::mesh::repair` report types.
 *
 * Produced by the WASM scene engine when a model is added, so it describes the
 * mesh the viewer is actually showing — and, because the engine runs the same
 * repair code when it slices, the mesh that will be printed.
 */

/** Topological health of a mesh, measured on its welded triangle graph. */
export interface MeshDiagnostics {
  triangles: number;
  vertices: number;
  /** Connected components, joined across shared edges. */
  shells: number;
  /** Triangles with a repeated corner or effectively zero area. */
  degenerate_faces: number;
  /** Triangles that repeat another triangle's corner set. */
  duplicate_faces: number;
  /** Edges with more than two incident triangles. */
  non_manifold_edges: number;
  /** Edges with exactly one incident triangle that bound real area. */
  boundary_edges: number;
  /**
   * Open edges whose loop encloses *no* area — a zero-width slit (typically a
   * T-junction), not a hole. The surface bounds the same solid with or without
   * it and no patch can close it, so it is never treated as a defect.
   */
  slit_boundary_edges: number;
  /** Closed loops formed by the boundary edges, i.e. the number of holes. */
  holes: number;
  largest_hole_edges: number;
  /** Shared edges whose two triangles disagree about which side is outside. */
  inconsistent_winding_edges: number;
  /** Closed shells whose normals point inward. */
  inverted_shells: number;
}

/** What the repair pass changed. */
export interface MeshRepairActions {
  welded_vertices: number;
  removed_degenerate_faces: number;
  removed_duplicate_faces: number;
  flipped_faces: number;
  filled_holes: number;
  added_fill_triangles: number;
  /** Holes left open because they were larger than the cap limit. */
  unfilled_holes: number;
}

/** Health before, health after, and the actions taken in between. */
export interface MeshReport {
  before: MeshDiagnostics;
  after: MeshDiagnostics;
  actions: MeshRepairActions;
  /** True when the mesh was rewritten. */
  repaired: boolean;
  /** One-line human summary, worded identically by every runtime. */
  summary: string;
}

/** True when the incoming model had nothing wrong with it. */
export function meshWasClean(report: MeshReport): boolean {
  return !meshHasDefects(report.before);
}

/** True when defects survived the repair pass. */
export function meshHasRemainingDefects(report: MeshReport): boolean {
  return meshHasDefects(report.after);
}

/** True when the report is worth putting in front of the user. */
export function meshReportIsNoteworthy(report: MeshReport): boolean {
  return !meshWasClean(report) || meshHasRemainingDefects(report);
}

function meshHasDefects(d: MeshDiagnostics): boolean {
  return (
    d.degenerate_faces > 0 ||
    d.duplicate_faces > 0 ||
    d.inverted_shells > 0 ||
    d.boundary_edges > 0 ||
    d.non_manifold_edges > 0 ||
    d.inconsistent_winding_edges > 0
  );
}

/**
 * Short list of what the repair pass fixed, phrased for a toast body.
 * Returns `null` when nothing was changed.
 */
export function describeMeshRepairs(actions: MeshRepairActions): string | null {
  const parts: string[] = [];
  const add = (count: number, singular: string, plural: string, verb: string) => {
    if (count > 0) {
      parts.push(`${verb} ${count} ${count === 1 ? singular : plural}`);
    }
  };
  add(actions.filled_holes, 'hole', 'holes', 'filled');
  add(actions.welded_vertices, 'split vertex', 'split vertices', 'welded');
  add(actions.flipped_faces, 'triangle', 'triangles', 'flipped');
  add(actions.removed_degenerate_faces, 'zero-area triangle', 'zero-area triangles', 'removed');
  add(actions.removed_duplicate_faces, 'duplicate', 'duplicates', 'removed');
  return parts.length > 0 ? parts.join(', ') : null;
}

/**
 * Short list of the defects that could not be repaired, phrased for a toast
 * body. Returns `null` when the mesh came out clean.
 */
export function describeMeshDefects(d: MeshDiagnostics): string | null {
  const parts: string[] = [];
  const add = (count: number, singular: string, plural: string) => {
    if (count > 0) {
      parts.push(`${count} ${count === 1 ? singular : plural}`);
    }
  };
  add(d.non_manifold_edges, 'non-manifold edge', 'non-manifold edges');
  add(d.holes, 'unfilled hole', 'unfilled holes');
  add(d.inconsistent_winding_edges, 'flipped edge', 'flipped edges');
  add(d.degenerate_faces, 'zero-area triangle', 'zero-area triangles');
  add(d.duplicate_faces, 'duplicate triangle', 'duplicate triangles');
  add(d.inverted_shells, 'inside-out shell', 'inside-out shells');
  return parts.length > 0 ? parts.join(', ') : null;
}
