import type { Group, InstancedMesh } from 'three';
import {
  ROLE_COLORS_DARK,
  type ColorChannel,
  type RoleColorPalette,
  type RoleName,
  type ScalarRange,
} from '../../services/gcode-preview';
import {
  applyHiddenRoles,
  applyLayerVisibility,
  buildGcodeModel,
  disposeGcodeModel,
  type GcodeLayerSource,
  type GcodeModel,
  maxExtrusionWidth,
  setJointsVisible,
  tagInstanceRefs,
  updateViewColors,
} from './gcode-layer-renderer';

/**
 * Minimal interface for the WASM-side handle that exposes G-code layer data.
 * Only the fields consumed by GcodeOrchestrator are declared here; the actual
 * WASM object may carry additional members.
 */
export type GcodeSource = GcodeLayerSource;

/**
 * Owns the Three.js geometry produced from a WASM G-code handle.
 *
 * All geometry is built from data returned by `GcodeSource` (i.e. the WASM
 * SceneHandle); Three.js is only responsible for layer/segment visibility
 * and role filtering.  No geometry is constructed inside this class.
 *
 * The model is stored as one instanced buffer pair *per role* spanning every
 * layer (see {@link buildGcodeModel}), so frame cost stays flat as plates grow
 * instead of scaling with layer count.
 */
export class GcodeOrchestrator {
  private model: GcodeModel | null = null;
  private lastMin = 0;
  private lastMax = 0;
  private lastProgress = 1;
  private beadWidth = 0.4;
  private jointsOn = true;

  constructor(private readonly contentRoot: Group) {}

  get count(): number {
    return this.model?.layers.length ?? 0;
  }

  get totalSegments(): number {
    return this.model?.totalSegments ?? 0;
  }

  /** Widest extrusion in the current model (mm); drives the joint LOD. */
  get extrusionWidth(): number {
    return this.beadWidth;
  }

  /**
   * Cylinder meshes of currently-visible roles, for the hover probe's raycast.
   * Skips hidden roles and empty draw ranges so the raycast only considers what
   * the user can actually see.
   */
  hoverableMeshes(): InstancedMesh[] {
    const out: InstancedMesh[] = [];
    if (!this.model) {
      return out;
    }
    for (const rs of this.model.roleSegments) {
      if (rs.mesh?.visible && rs.mesh.count > 0) {
        out.push(rs.mesh);
      }
    }
    return out;
  }

  /**
   * Build the Three.js geometry for every layer in the handle and add it to the
   * content root.  Any previously built model is disposed first.
   */
  buildFromHandle(
    handle: GcodeSource,
    colors: RoleColorPalette = ROLE_COLORS_DARK,
  ): { totalSegments: number } {
    this.dispose();

    const model = buildGcodeModel(handle, colors);
    tagInstanceRefs(model);
    this.contentRoot.add(model.group);
    this.model = model;
    this.beadWidth = maxExtrusionWidth(model);
    this.lastMin = 0;
    this.lastMax = Math.max(0, model.layers.length - 1);
    this.lastProgress = 1;
    this.jointsOn = true;
    return { totalSegments: model.totalSegments };
  }

  /**
   * Show only layers whose index falls within `[min, max]`, preserving the
   * current scrub position on the top layer.
   */
  showRange(min: number, max: number): void {
    if (!this.model) {
      return;
    }
    this.lastMin = min;
    this.lastMax = max;
    applyLayerVisibility(this.model, min, max, this.lastProgress);
  }

  /**
   * Scrub through the segments of layer `topIndex`.
   * `progress` is a fraction [0, 1] of that layer's total segment count.
   */
  applyProgress(topIndex: number, progress: number): void {
    if (!this.model) {
      return;
    }
    this.lastMax = topIndex;
    this.lastProgress = progress;
    applyLayerVisibility(this.model, this.lastMin, topIndex, progress);
  }

  /**
   * Recolor all layers for the current view mode (category, a segment scalar,
   * or a per-layer scalar) and palette. Call this when the theme, view mode,
   * scalar range, selected fan, or legend hover-band changes.
   */
  applyView(
    colors: RoleColorPalette,
    channel: ColorChannel | null,
    range: ScalarRange,
    fanKey: string | null,
    band: { lo: number; hi: number } | null = null,
  ): void {
    if (!this.model) {
      return;
    }
    updateViewColors(this.model, colors, channel, range, fanKey, band);
  }

  /**
   * Hide all segments belonging to the given roles.
   */
  applyHiddenRoles(hidden: ReadonlySet<RoleName>): void {
    if (!this.model) {
      return;
    }
    applyHiddenRoles(this.model, hidden);
    // Role visibility owns the joint meshes too, so re-assert the LOD state.
    if (!this.jointsOn) {
      setJointsVisible(this.model, false);
    }
  }

  /**
   * Turn corner joint balls on/off. Returns `true` when the state changed, so
   * the caller can request a redraw only when it matters.
   */
  setJointsVisible(visible: boolean): boolean {
    if (!this.model || this.jointsOn === visible) {
      return false;
    }
    this.jointsOn = visible;
    setJointsVisible(this.model, visible);
    return true;
  }

  /**
   * Remove the model from the content root and release its Three.js resources.
   */
  dispose(): void {
    if (!this.model) {
      return;
    }
    this.contentRoot.remove(this.model.group);
    disposeGcodeModel(this.model);
    this.model = null;
    this.lastMin = 0;
    this.lastMax = 0;
    this.lastProgress = 1;
  }
}
