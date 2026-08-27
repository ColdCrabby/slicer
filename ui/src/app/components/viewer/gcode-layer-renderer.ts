import {
  BufferAttribute,
  BufferGeometry,
  Color,
  CylinderGeometry,
  Group,
  InstancedBufferAttribute,
  InstancedMesh,
  LineBasicMaterial,
  LineSegments,
  MeshStandardMaterial,
  Object3D,
  SphereGeometry,
  Vector3,
} from 'three';
import type { GcodeLayerBuffer } from '../../../generated/scene-wasm/scene_engine';
import {
  FLOATS_PER_SEGMENT,
  ROLE_COLORS_DARK,
  ROLE_ORDER,
  SPEED_OFFSET,
  sampleSpeedColor,
  type ColorChannel,
  type LayerScalarMeta,
  type RoleColorPalette,
  type RoleName,
  type ScalarRange,
} from '../../services/gcode-preview';

// -- Shared types -------------------------------------------------------------

/**
 * Gcode tubes are lit by the scene rig, but the preview palette is chosen to
 * match the flat legend swatches. To keep every tube reading as its legend
 * colour (crisp, theme-correct) instead of being darkened into a muddy brown by
 * the lighting, the role colour is emitted as self-illumination and only a
 * small fraction is left as diffuse — just enough to give the cylinders a hint
 * of form. This also makes the tube colour essentially independent of the scene
 * lighting, so the model-oriented light rig can be tuned without washing out or
 * darkening the preview.
 */
const EXTRUSION_EMISSIVE_INTENSITY = 0.45;
const EXTRUSION_DIFFUSE_TINT = 0.9;

export interface RoleSegments {
  role: RoleName;
  mesh?: InstancedMesh;
  joints?: InstancedMesh;
  lines?: LineSegments;
  /** Number of line segments */
  count: number;
  /**
   * Per-segment extrusion width, height and speed (length === `count`).
   * Present only for extrusion roles (not travel or seam) so scalar view modes
   * can recolor via a {@link ScalarChannel} without re-reading the WASM buffer.
   */
  widths?: Float32Array;
  heights?: Float32Array;
  speeds?: Float32Array;
  /**
   * Per-instance opacity attributes (segment cylinders and their joints) used
   * to fade out-of-band extrusions while hovering the legend. `1` = opaque.
   */
  meshOpacity?: InstancedBufferAttribute;
  jointsOpacity?: InstancedBufferAttribute;
}

export interface LayerInfo {
  index: number;
  z: number;
  group: Group;
  totalSegments: number;
  roleSegments: RoleSegments[];
  blockLayout: { role: RoleName; count: number }[];
  /** Per-layer machine state for the fan / temperature / layer-time views. */
  meta: LayerScalarMeta;
}

// -- Layer builder ------------------------------------------------------------

interface LayerBuild {
  group: Group;
  totalSegments: number;
  roleSegments: RoleSegments[];
  blockLayout: { role: RoleName; count: number }[];
  meta: LayerScalarMeta;
}

/** Read the per-layer machine-state metadata out of a WASM layer buffer. */
function readLayerMeta(buf: GcodeLayerBuffer): LayerScalarMeta {
  const fans = new Map<string, number>();
  const fanCount = buf.fanCount();
  for (let i = 0; i < fanCount; i++) {
    fans.set(buf.fanKey(i), buf.fanSpeed(i));
  }
  return {
    nozzleTemp: buf.nozzleTemp(),
    tool: buf.tool,
    layerTimeS: buf.layerTimeS(),
    fans,
  };
}

/** Back-reference stashed on each cylinder InstancedMesh for hover lookup. */
export interface GcodeInstanceRef {
  roleSegments: RoleSegments;
  layerIndex: number;
  z: number;
  meta: LayerScalarMeta;
}

/** `userData` key under which the {@link GcodeInstanceRef} is stored. */
export const GCODE_REF_KEY = 'gcodeRef';

/** Tag every cylinder mesh of a layer so a raycast `instanceId` maps to a value. */
export function tagInstanceRefs(info: LayerInfo): void {
  for (const rs of info.roleSegments) {
    if (rs.mesh) {
      rs.mesh.userData[GCODE_REF_KEY] = {
        roleSegments: rs,
        layerIndex: info.index,
        z: info.z,
        meta: info.meta,
      } satisfies GcodeInstanceRef;
    }
  }
}

const ROLE_ID_TO_NAME: Record<number, RoleName> = {
  0: 'outerWall',
  1: 'innerWall',
  2: 'infill',
  3: 'topSurface',
  4: 'bottomSurface',
  5: 'travel',
  6: 'other',
  7: 'bridge',
  8: 'skirt',
  9: 'support',
  10: 'seam',
  11: 'overhangPerimeter',
  12: 'gapFill',
  13: 'solidInfill',
  14: 'supportInterface',
  15: 'brim',
  16: 'primeTower',
  17: 'internalBridge',
};

const _dummy = new Object3D();
const _p0 = new Vector3();
const _p1 = new Vector3();
const _mid = new Vector3();

/** Brightness multiplier applied to extrusions outside the legend hover-band. */
const OUT_OF_BAND_DIM = 0.16;

/** Opacity applied to extrusions outside the legend hover-band (see-through). */
const OUT_OF_BAND_ALPHA = 0.12;

// Reusable geometries. We will scale instances.
//
// Tessellation is deliberately low. Every extrusion segment instances one tube
// body plus one joint ball, so at millions of segments the per-primitive
// triangle count dominates the frame budget. A bead is well under a millimetre
// wide, so a joint ball is almost always sub-pixel or inscribed inside the tube
// (invisible on straight runs) — an 8x8 sphere there was pure waste. 6x4 keeps
// the silhouette round at the bends that actually show it while cutting joint
// triangles ~3x.
const segmentGeometry = new CylinderGeometry(0.5, 0.5, 1, 8, 1, false);
segmentGeometry.rotateX(Math.PI / 2); // Align along Z
const jointGeometry = new SphereGeometry(0.5, 6, 4);

// Seam dots are rendered as larger spheres.  We keep a dedicated geometry so
// they can be rendered independently of the normal joint spheres.
const seamDotGeometry = new SphereGeometry(0.5, 8, 6);

/**
 * Inject a per-instance opacity (`aOpacity`) multiply into a standard material
 * so individual extrusions can fade independently — Three.js `InstancedMesh`
 * has per-instance color but no per-instance alpha out of the box.
 */
function installInstanceOpacity(material: MeshStandardMaterial): void {
  material.onBeforeCompile = (shader) => {
    shader.vertexShader =
      'attribute float aOpacity;\nvarying float vOpacity;\n' +
      shader.vertexShader.replace(
        '#include <begin_vertex>',
        '#include <begin_vertex>\n  vOpacity = aOpacity;',
      );
    shader.fragmentShader =
      'varying float vOpacity;\n' +
      shader.fragmentShader.replace(
        '#include <dithering_fragment>',
        '#include <dithering_fragment>\n  gl_FragColor.a *= vOpacity;',
      );
  };
}

export function buildLayerGroup(
  buf: GcodeLayerBuffer,
  colors: RoleColorPalette = ROLE_COLORS_DARK,
): LayerBuild {
  const group = new Group();
  group.userData['handle'] = buf;

  const numBlocks = buf.blocksCount();
  const roleTotals: Record<RoleName, number> = {
    outerWall: 0,
    innerWall: 0,
    overhangPerimeter: 0,
    infill: 0,
    solidInfill: 0,
    gapFill: 0,
    bridge: 0,
    internalBridge: 0,
    topSurface: 0,
    bottomSurface: 0,
    support: 0,
    supportInterface: 0,
    skirt: 0,
    brim: 0,
    primeTower: 0,
    travel: 0,
    other: 0,
    seam: 0,
  };

  const blockLayout: { role: RoleName; count: number }[] = [];
  let totalSegments = 0;

  // Pass 1: Tally counts to allocate exactly one buffer/mesh per role
  for (let b = 0; b < numBlocks; b++) {
    const roleId = buf.blockRole(b);
    const dataLen = buf.blockData(b).length;
    if (dataLen === 0) continue;

    const count = dataLen / FLOATS_PER_SEGMENT;
    const role = ROLE_ID_TO_NAME[roleId] || 'other';

    roleTotals[role] += count;
    blockLayout.push({ role, count });
    totalSegments += count;
  }

  const roleSegmentsMap: Partial<Record<RoleName, RoleSegments>> = {};

  // Pass 2: Allocate Three.js instances
  for (const role of ROLE_ORDER) {
    const count = roleTotals[role];
    if (count === 0) continue;

    const color = colors[role];

    if (role === 'travel') {
      const pts = new Float32Array(count * 6);
      const geometry = new BufferGeometry();
      geometry.setAttribute('position', new BufferAttribute(pts, 3));
      const material = new LineBasicMaterial({ color });
      const lines = new LineSegments(geometry, material);
      group.add(lines);
      roleSegmentsMap[role] = { role, lines, count };
    } else if (role === 'seam') {
      // Seam points are rendered as spheres — no cylinder body, just dots.
      const material = new MeshStandardMaterial({
        color,
        emissive: color,
        emissiveIntensity: EXTRUSION_EMISSIVE_INTENSITY,
        roughness: 0.3,
        metalness: 0.1,
      });
      const dots = new InstancedMesh(seamDotGeometry, material, count);
      dots.instanceMatrix.setUsage(35044 /* THREE.DynamicDrawUsage */);
      dots.count = count;
      group.add(dots);
      // Re-use the `joints` slot so existing visibility / progress logic works.
      roleSegmentsMap[role] = { role, joints: dots, count };
    } else {
      const material = new MeshStandardMaterial({
        color,
        emissive: color,
        emissiveIntensity: EXTRUSION_EMISSIVE_INTENSITY,
        roughness: 0.6,
      });
      installInstanceOpacity(material);

      // Per-mesh geometry clones so each carries its own per-instance opacity
      // attribute (instanced attributes can't be shared across meshes).
      const segGeom = segmentGeometry.clone();
      const jointGeom = jointGeometry.clone();
      const meshOpacity = new InstancedBufferAttribute(new Float32Array(count).fill(1), 1);
      // One joint ball per segment, placed at its start point. Consecutive
      // segments of a path share a vertex, so a ball at every start already
      // rounds every interior joint; the path's final vertex is closed by the
      // capped tube. This halves joint instances vs. one ball per endpoint.
      const jointsOpacity = new InstancedBufferAttribute(new Float32Array(count).fill(1), 1);
      meshOpacity.setUsage(35044 /* THREE.DynamicDrawUsage */);
      jointsOpacity.setUsage(35044 /* THREE.DynamicDrawUsage */);
      segGeom.setAttribute('aOpacity', meshOpacity);
      jointGeom.setAttribute('aOpacity', jointsOpacity);

      const mesh = new InstancedMesh(segGeom, material, count);
      const joints = new InstancedMesh(jointGeom, material, count);
      mesh.instanceMatrix.setUsage(35044 /* THREE.DynamicDrawUsage */);
      joints.instanceMatrix.setUsage(35044 /* THREE.DynamicDrawUsage */);

      mesh.count = count;
      joints.count = count;
      group.add(mesh);
      group.add(joints);

      roleSegmentsMap[role] = {
        role,
        mesh,
        joints,
        count,
        widths: new Float32Array(count),
        heights: new Float32Array(count),
        speeds: new Float32Array(count),
        meshOpacity,
        jointsOpacity,
      };
    }
  }

  // Pass 3: Fill matrices and buffers sequentially per role
  const roleOffsets: Record<RoleName, number> = {
    outerWall: 0,
    innerWall: 0,
    overhangPerimeter: 0,
    infill: 0,
    solidInfill: 0,
    gapFill: 0,
    bridge: 0,
    internalBridge: 0,
    topSurface: 0,
    bottomSurface: 0,
    support: 0,
    supportInterface: 0,
    skirt: 0,
    brim: 0,
    primeTower: 0,
    travel: 0,
    other: 0,
    seam: 0,
  };

  for (let b = 0; b < numBlocks; b++) {
    const data = buf.blockData(b);
    if (data.length === 0) continue;

    const count = data.length / FLOATS_PER_SEGMENT;
    const roleId = buf.blockRole(b);
    const role = ROLE_ID_TO_NAME[roleId] || 'other';

    const rs = roleSegmentsMap[role];
    if (!rs) continue;

    const baseOffset = roleOffsets[role];

    if (role === 'travel') {
      const pts = (rs.lines!.geometry.getAttribute('position') as BufferAttribute)
        .array as Float32Array;
      for (let i = 0; i < count; i++) {
        const off = i * FLOATS_PER_SEGMENT;
        const pOff = (baseOffset + i) * 6;
        pts[pOff] = data[off];
        pts[pOff + 1] = data[off + 1];
        pts[pOff + 2] = data[off + 2];
        pts[pOff + 3] = data[off + 3];
        pts[pOff + 4] = data[off + 4];
        pts[pOff + 5] = data[off + 5];
      }
      rs.lines!.geometry.attributes['position'].needsUpdate = true;
    } else if (role === 'seam') {
      // Seam blocks are degenerate (p0 == p1).  Render as a scaled white dot
      // sphere positioned at p0.  The width field (data[6]) carries the dot
      // radius set to SEAM_DOT_RADIUS (0.6 mm) in the Rust parser.
      const dots = rs.joints!;
      for (let i = 0; i < count; i++) {
        const globalI = baseOffset + i;
        const offset = i * FLOATS_PER_SEGMENT;
        const dotSize = data[offset + 6] || 0.6; // 0.6 = SEAM_DOT_RADIUS in parser.rs
        _dummy.position.set(data[offset], data[offset + 1], data[offset + 2]);
        _dummy.rotation.set(0, 0, 0);
        _dummy.scale.set(dotSize, dotSize, dotSize);
        _dummy.updateMatrix();
        dots.setMatrixAt(globalI, _dummy.matrix);
      }
      dots.instanceMatrix.needsUpdate = true;
    } else {
      const mesh = rs.mesh!;
      const joints = rs.joints!;

      for (let i = 0; i < count; i++) {
        const globalI = baseOffset + i;
        const offset = i * FLOATS_PER_SEGMENT;

        _p0.set(data[offset], data[offset + 1], data[offset + 2]);
        _p1.set(data[offset + 3], data[offset + 4], data[offset + 5]);
        const width = data[offset + 6] || 0.4;
        const height = data[offset + 7] || 0.2;
        if (rs.widths) rs.widths[globalI] = width;
        if (rs.heights) rs.heights[globalI] = height;
        if (rs.speeds) rs.speeds[globalI] = data[offset + SPEED_OFFSET];

        const length = _p0.distanceTo(_p1);
        _mid.addVectors(_p0, _p1).multiplyScalar(0.5);

        _dummy.position.copy(_mid);
        _dummy.up.set(0, 0, 1);
        _dummy.lookAt(_p1);

        _dummy.scale.set(width, height, length || 0.001);
        _dummy.updateMatrix();
        mesh.setMatrixAt(globalI, _dummy.matrix);

        _dummy.scale.set(width, height, width);
        _dummy.position.copy(_p0);
        _dummy.updateMatrix();
        joints.setMatrixAt(globalI, _dummy.matrix);
      }
      mesh.instanceMatrix.needsUpdate = true;
      joints.instanceMatrix.needsUpdate = true;
    }

    roleOffsets[role] += count;
  }

  const roleSegments: RoleSegments[] = Object.values(roleSegmentsMap) as RoleSegments[];

  return { group, totalSegments, roleSegments, blockLayout, meta: readLayerMeta(buf) };
}

export function disposeLayerGroup(group: Group): void {
  for (const child of group.children) {
    if (child instanceof InstancedMesh || child instanceof LineSegments) {
      child.geometry.dispose();
      if (Array.isArray(child.material)) {
        for (const m of child.material) m.dispose();
      } else {
        child.material.dispose();
      }
    }
  }
}

/**
 * Recolor all built layers for the current view mode without rebuilding any
 * geometry. Called when the theme, view mode, scalar range, selected fan, or
 * legend hover-band changes.
 *
 * - `channel === null` (category): every segment takes its role color.
 * - a *segment* channel: extrusion segments are colored per-instance by the
 *   channel value (speed, flow, width, height).
 * - a *layer* channel (fan / temperature / layer time): every segment in a
 *   layer shares one color derived from that layer's value; layers with no
 *   data for the channel fall back to their role color.
 * When `band` is set, values outside `[band.lo, band.hi]` are dimmed so the
 * legend-hovered range stands out. Travel and seam markers always keep their
 * role color.
 */
export function updateViewColors(
  layers: LayerInfo[],
  colors: RoleColorPalette,
  channel: ColorChannel | null,
  range: ScalarRange,
  fanKey: string | null,
  band: { lo: number; hi: number } | null = null,
): void {
  const span = range.max - range.min;
  const c = new Color();
  const bandActive = band !== null;
  const outOfBand = (v: number): boolean => band !== null && (v < band.lo || v > band.hi);
  for (const info of layers) {
    // Per-layer channels resolve to a single value shared by every segment.
    const layerValue =
      channel && channel.scope === 'layer' ? channel.extractLayer(info.meta, fanKey) : null;

    for (const rs of info.roleSegments) {
      if (rs.role === 'travel') {
        if (rs.lines) (rs.lines.material as LineBasicMaterial).color.set(colors.travel);
        continue;
      }
      if (rs.role === 'seam') {
        if (rs.joints) {
          const m = rs.joints.material as MeshStandardMaterial;
          c.set(colors.seam);
          m.emissive.copy(c);
          m.color.copy(c).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
        }
        continue;
      }

      const { mesh, joints, widths, heights, speeds, count } = rs;

      if (channel && channel.scope === 'segment' && mesh && widths && heights && speeds) {
        // Per-instance color; keep the material white (and unlit emissive off)
        // so the per-instance scalar tint shows unmodulated.
        const mm = mesh.material as MeshStandardMaterial;
        mm.color.set(0xffffff);
        mm.emissive.setHex(0x000000);
        ensureInstanceColor(mesh, count);
        if (joints) {
          const jm = joints.material as MeshStandardMaterial;
          jm.color.set(0xffffff);
          jm.emissive.setHex(0x000000);
          ensureInstanceColor(joints, count);
        }
        const meshAlpha = rs.meshOpacity?.array as Float32Array | undefined;
        const jointAlpha = rs.jointsOpacity?.array as Float32Array | undefined;
        for (let i = 0; i < count; i++) {
          const value = channel.extract(widths[i], heights[i], speeds[i]);
          const t = span > 0 ? (value - range.min) / span : 0.5;
          c.set(sampleSpeedColor(t));
          const dim = outOfBand(value);
          if (dim) c.multiplyScalar(OUT_OF_BAND_DIM);
          mesh.setColorAt(i, c);
          const alpha = bandActive && dim ? OUT_OF_BAND_ALPHA : 1;
          if (meshAlpha) meshAlpha[i] = alpha;
          if (joints) {
            joints.setColorAt(i, c);
          }
          if (jointAlpha) {
            jointAlpha[i] = alpha;
          }
        }
        if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
        if (joints?.instanceColor) joints.instanceColor.needsUpdate = true;
        if (rs.meshOpacity) rs.meshOpacity.needsUpdate = true;
        if (rs.jointsOpacity) rs.jointsOpacity.needsUpdate = true;
        applyMeshTransparency(rs, bandActive);
      } else if (channel && channel.scope === 'layer' && layerValue !== null) {
        // One constant color for the whole layer via the shared material.
        const t = span > 0 ? (layerValue - range.min) / span : 0.5;
        const dim = outOfBand(layerValue);
        c.set(sampleSpeedColor(t));
        if (dim) c.multiplyScalar(OUT_OF_BAND_DIM);
        if (mesh) {
          const m = mesh.material as MeshStandardMaterial;
          m.emissive.copy(c);
          m.color.copy(c).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
          resetInstanceColor(mesh);
        }
        if (joints) {
          const m = joints.material as MeshStandardMaterial;
          m.emissive.copy(c);
          m.color.copy(c).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
          resetInstanceColor(joints);
        }
        fillOpacity(rs.meshOpacity, bandActive && dim ? OUT_OF_BAND_ALPHA : 1);
        fillOpacity(rs.jointsOpacity, bandActive && dim ? OUT_OF_BAND_ALPHA : 1);
        applyMeshTransparency(rs, bandActive);
      } else {
        // Category, or a layer channel with no data here: role color, and
        // neutralize any leftover per-instance scalar tint / transparency.
        c.set(colors[rs.role]);
        if (mesh) {
          const m = mesh.material as MeshStandardMaterial;
          m.emissive.copy(c);
          m.color.copy(c).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
          resetInstanceColor(mesh);
        }
        if (joints) {
          const m = joints.material as MeshStandardMaterial;
          m.emissive.copy(c);
          m.color.copy(c).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
          resetInstanceColor(joints);
        }
        applyMeshTransparency(rs, false);
      }
    }
  }
}

/** Allocate a full-capacity white instance-color buffer if one is missing. */
function ensureInstanceColor(mesh: InstancedMesh, capacity: number): void {
  if (!mesh.instanceColor || mesh.instanceColor.count < capacity) {
    mesh.instanceColor = new InstancedBufferAttribute(new Float32Array(capacity * 3).fill(1), 3);
  }
}

/** Reset every instance color back to white so material color shows through. */
function resetInstanceColor(mesh: InstancedMesh): void {
  const attr = mesh.instanceColor;
  if (!attr) return;
  (attr.array as Float32Array).fill(1);
  attr.needsUpdate = true;
}

/** Fill a per-instance opacity attribute with a single value. */
function fillOpacity(attr: InstancedBufferAttribute | undefined, value: number): void {
  if (!attr) return;
  (attr.array as Float32Array).fill(value);
  attr.needsUpdate = true;
}

/**
 * Move a role's shared material in/out of the transparent pass. `depthWrite` is
 * disabled while transparent so faded out-of-band extrusions don't occlude the
 * in-band ones behind them; non-band stays fully opaque (no regression).
 */
function applyMeshTransparency(rs: RoleSegments, transparent: boolean): void {
  const material = (rs.mesh?.material ?? rs.joints?.material) as MeshStandardMaterial | undefined;
  if (!material) return;
  material.depthWrite = !transparent;
  if (material.transparent !== transparent) {
    material.transparent = transparent;
  }
}

export function showLayerRange(
  layers: LayerInfo[],
  min: number,
  max: number,
  prevMax: number,
): void {
  const prevInfo = layers[prevMax];
  if (prevInfo && prevMax !== max) {
    for (const rs of prevInfo.roleSegments) {
      if (rs.role === 'seam') {
        if (rs.joints) rs.joints.count = rs.count;
      } else {
        if (rs.mesh) rs.mesh.count = rs.count;
        if (rs.joints) rs.joints.count = rs.count;
        if (rs.lines) rs.lines.geometry.setDrawRange(0, Infinity);
      }
    }
  }

  for (const info of layers) {
    info.group.visible = info.index >= min && info.index <= max;
  }
}

export function applySegmentProgress(
  layers: LayerInfo[],
  topIndex: number,
  progress: number,
): void {
  const info = layers[topIndex];
  if (!info) return;

  const target = Math.round(progress * info.totalSegments);

  const visibleCounts: Record<RoleName, number> = {
    outerWall: 0,
    innerWall: 0,
    overhangPerimeter: 0,
    infill: 0,
    solidInfill: 0,
    gapFill: 0,
    bridge: 0,
    internalBridge: 0,
    topSurface: 0,
    bottomSurface: 0,
    support: 0,
    supportInterface: 0,
    skirt: 0,
    brim: 0,
    primeTower: 0,
    travel: 0,
    other: 0,
    seam: 0,
  };

  let remaining = target;
  for (const block of info.blockLayout) {
    const show = Math.min(remaining, block.count);
    visibleCounts[block.role] += show;
    remaining -= show;
    if (remaining <= 0) break;
  }

  for (const rs of info.roleSegments) {
    const show = visibleCounts[rs.role] || 0;
    if (rs.role === 'seam') {
      // Seam dots use only the joints slot (no cylinder mesh).
      if (rs.joints) rs.joints.count = show;
    } else {
      if (rs.mesh) rs.mesh.count = show;
      if (rs.joints) rs.joints.count = show;
      if (rs.lines) rs.lines.geometry.setDrawRange(0, show * 2);
    }
  }
}

export function applyHiddenRoles(layers: LayerInfo[], hiddenRoles: ReadonlySet<RoleName>): void {
  for (const info of layers) {
    for (const rs of info.roleSegments) {
      const visible = !hiddenRoles.has(rs.role);
      if (rs.mesh) rs.mesh.visible = visible;
      if (rs.joints) rs.joints.visible = visible;
      if (rs.lines) rs.lines.visible = visible;
    }
  }
}
