import type { Group } from 'three';
import type { GcodeLayerBuffer } from '../../../generated/scene-wasm/scene_engine';
import {
  ROLE_COLORS_DARK,
  type ColorChannel,
  type RoleColorPalette,
  type RoleName,
  type ScalarRange,
} from '../../services/gcode-preview';
import {
  applyHiddenRoles,
  applySegmentProgress,
  buildLayerGroup,
  disposeLayerGroup,
  type LayerInfo,
  updateViewColors,
} from './gcode-layer-renderer';

/**
 * Minimal interface for the WASM-side handle that exposes G-code layer data.
 * Only the fields consumed by GcodeOrchestrator are declared here; the actual
 * WASM object may carry additional members.
 */
export interface GcodeSource {
  layerCount(): number;
  getLayer(index: number): GcodeLayerBuffer;
}

/**
 * Owns the Three.js layer groups produced from a WASM G-code handle.
 *
 * All geometry is built from data returned by `GcodeSource` (i.e. the WASM
 * SceneHandle); Three.js is only responsible for layer/segment visibility
 * and role filtering.  No geometry is constructed inside this class.
 */
export class GcodeOrchestrator {
  private layers: LayerInfo[] = [];
  private prevMaxLayer = 0;
  private _totalSegments = 0;

  constructor(private readonly contentRoot: Group) {}

  get count(): number {
    return this.layers.length;
  }

  get totalSegments(): number {
    return this._totalSegments;
  }

  /**
   * Build Three.js line-segment groups for every layer in the handle and
   * add them to the content root.  Any previously built layers are disposed
   * first.
   */
  buildFromHandle(
    handle: GcodeSource,
    colors: RoleColorPalette = ROLE_COLORS_DARK,
  ): { totalSegments: number } {
    this.dispose();

    const count = handle.layerCount();
    let total = 0;

    for (let i = 0; i < count; i++) {
      const buf = handle.getLayer(i);
      const built = buildLayerGroup(buf, colors);
      const info: LayerInfo = {
        index: i,
        z: buf.z ?? i,
        group: built.group,
        totalSegments: built.totalSegments,
        roleSegments: built.roleSegments,
        blockLayout: built.blockLayout,
        meta: built.meta,
      };
      this.layers.push(info);
      this.contentRoot.add(built.group);
      total += built.totalSegments;
    }

    this._totalSegments = total;
    return { totalSegments: total };
  }

  /**
   * Show only layers whose index falls within `[min, max]`.
   */
  showRange(min: number, max: number): void {
    if (this.layers.length === 0) {
      return;
    }
    // Restore draw range on the previous top layer before switching.
    applySegmentProgress(this.layers, this.prevMaxLayer, 1);
    showLayerRange(this.layers, min, max, this.prevMaxLayer);
    this.prevMaxLayer = max;
  }

  /**
   * Scrub through the segments of layer `topIndex`.
   * `progress` is a fraction [0, 1] of that layer's total segment count.
   */
  applyProgress(topIndex: number, progress: number): void {
    applySegmentProgress(this.layers, topIndex, progress);
  }

  /**
   * Recolor all layers for the current view mode (category, a segment scalar,
   * or a per-layer scalar) and palette. Call this when the theme, view mode,
   * scalar range, or selected fan changes.
   */
  applyView(
    colors: RoleColorPalette,
    channel: ColorChannel | null,
    range: ScalarRange,
    fanKey: string | null,
  ): void {
    updateViewColors(this.layers, colors, channel, range, fanKey);
  }

  /**
   * Hide all segments belonging to the given roles across all layers.
   */
  applyHiddenRoles(hidden: ReadonlySet<RoleName>): void {
    applyHiddenRoles(this.layers, hidden);
  }

  /**
   * Remove all layer groups from the content root and release their
   * Three.js resources.
   */
  dispose(): void {
    for (const info of this.layers) {
      this.contentRoot.remove(info.group);
      disposeLayerGroup(info.group);
    }
    this.layers = [];
    this.prevMaxLayer = 0;
    this._totalSegments = 0;
  }
}

// ---------------------------------------------------------------------------
// Local re-implementation of showLayerRange to avoid mutating prevMax inside
// the renderer module.
// ---------------------------------------------------------------------------

function showLayerRange(layers: LayerInfo[], min: number, max: number, prevMax: number): void {
  const prevInfo = layers[prevMax];
  if (prevInfo && prevMax !== max) {
    for (const rs of prevInfo.roleSegments) {
      if (rs.mesh) rs.mesh.count = rs.count;
      if (rs.joints) rs.joints.count = rs.count * 2;
      if (rs.lines) rs.lines.geometry.setDrawRange(0, Infinity);
    }
  }
  for (const info of layers) {
    info.group.visible = info.index >= min && info.index <= max;
  }
}
