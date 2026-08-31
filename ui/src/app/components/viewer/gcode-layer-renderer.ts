import {
  BufferAttribute,
  BufferGeometry,
  Color,
  CylinderGeometry,
  Group,
  InstancedBufferAttribute,
  InstancedMesh,
  type Intersection,
  LineBasicMaterial,
  LineSegments,
  Matrix4,
  Mesh,
  MeshPhongMaterial,
  Object3D,
  type Raycaster,
  Sphere,
  SphereGeometry,
  Vector3,
} from 'three';
import type { GcodeLayerBuffer } from '../../../generated/scene-wasm/scene_engine';
import {
  ACCEL_OFFSET,
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
 *
 * The tubes use {@link MeshPhongMaterial} rather than {@link MeshStandardMaterial}:
 * the preview is overdraw-heavy (millions of instanced tubes redrawn every
 * frame), and a full PBR BRDF + IBL per fragment is wasted here because the look
 * is dominated by flat emissive. Phong keeps per-fragment shading (so the
 * low-poly 8-radial tubes and 6x4 joint balls stay smooth — Lambert's per-vertex
 * lighting would facet them) at a fraction of the fragment cost, and `specular`
 * is left black so the matte, mostly-emissive look is preserved.
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
   * Instance offset of the first segment belonging to each layer, length
   * `layerCount + 1` (so `layerStart[i + 1] - layerStart[i]` is layer `i`'s
   * share and `layerStart[layerCount] === count`).
   *
   * Instances are packed layer-ascending, which is what lets the layer slider
   * and the progress scrub reduce to a single `count` prefix instead of one
   * draw call per layer.
   */
  layerStart: Int32Array;
  /** Live `uLayerMin` uniform for this role's material (lower layer bound). */
  layerMinUniform: { value: number };
  /** Full-detail tube geometry (octagonal, capped), swapped in when zoomed in. */
  meshGeomHigh?: BufferGeometry;
  /** Low-detail tube geometry (open box), the default for large plates. */
  meshGeomLow?: BufferGeometry;
  /**
   * Per-segment extrusion width, height, speed and acceleration
   * (length === `count`). Present only for extrusion roles (not travel or seam)
   * so scalar view modes can recolor via a {@link ScalarChannel} without
   * re-reading the WASM buffer.
   */
  widths?: Float32Array;
  heights?: Float32Array;
  speeds?: Float32Array;
  accels?: Float32Array;
  /**
   * Per-instance opacity attributes (segment cylinders and their joints) used
   * to fade out-of-band extrusions while hovering the legend. `1` = opaque.
   */
  meshOpacity?: InstancedBufferAttribute;
  jointsOpacity?: InstancedBufferAttribute;
}

/**
 * Per-layer bookkeeping. Layers no longer own Three.js objects — geometry for
 * every layer lives in the shared per-role buffers — so this is pure metadata
 * used to resolve the layer slider, the progress scrub and hover tooltips.
 */
export interface LayerInfo {
  index: number;
  z: number;
  totalSegments: number;
  blockLayout: { role: RoleName; count: number }[];
  /** Per-layer machine state for the fan / temperature / layer-time views. */
  meta: LayerScalarMeta;
}

/**
 * The whole G-code preview: one {@link RoleSegments} per role, each spanning
 * every layer, plus the per-layer metadata needed to address into them.
 *
 * Keeping one mesh pair per *role* rather than per *layer × role* is what keeps
 * the frame cost flat as plates grow: a 335-layer plate went from ~2.5 k draw
 * calls (one pair per layer per role, each with its own material and shader
 * program) to ~18.
 */
export interface GcodeModel {
  group: Group;
  layers: LayerInfo[];
  roleSegments: RoleSegments[];
  totalSegments: number;
  /**
   * Segments actually submitted for the current layer range and scrub.
   *
   * This — not `totalSegments` — is what a frame costs, so it is what the
   * detail budget is measured against: isolating one layer of a huge plate is
   * cheap and can afford full detail.
   */
  visibleSegments: number;
}

// -- Model builder ------------------------------------------------------------

/** Minimal shape of the WASM handle the builder reads layers from. */
export interface GcodeLayerSource {
  layerCount(): number;
  getLayer(index: number): GcodeLayerBuffer;
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

/** Where a hovered instance sits in the plate. */
export interface GcodeInstanceLocation {
  layerIndex: number;
  z: number;
  meta: LayerScalarMeta;
}

/**
 * Back-reference stashed on each cylinder InstancedMesh for hover lookup.
 *
 * A role's buffer spans every layer, so the layer can no longer be baked into
 * the tag — {@link GcodeInstanceRef.resolve} maps a raycast `instanceId` back to
 * its layer via the role's `layerStart` index.
 */
export interface GcodeInstanceRef {
  roleSegments: RoleSegments;
  resolve(instanceId: number): GcodeInstanceLocation;
}

/** `userData` key under which the {@link GcodeInstanceRef} is stored. */
export const GCODE_REF_KEY = 'gcodeRef';

/** `userData` key holding the first instance index the raycast may consider. */
const RAYCAST_START_KEY = 'gcodeRaycastStart';

const _rcLocal = new Matrix4();
const _rcWorld = new Matrix4();
const _rcSphere = new Sphere();
const _rcProbe = new Mesh();
const _rcHits: Intersection[] = [];

/**
 * `InstancedMesh.raycast` that starts at a given instance instead of zero.
 *
 * The layer *upper* bound is a draw-range prefix that Three.js already honours
 * via `count`, but the *lower* bound lives in the vertex shader and is
 * therefore invisible to a raycast — without this, hovering in "single layer"
 * mode could report a segment from a hidden layer, and would scan the entire
 * plate to do it. Mirrors the stock implementation, only the loop start moves.
 */
function raycastFromInstance(
  this: InstancedMesh,
  raycaster: Raycaster,
  intersects: Intersection[],
): void {
  const matrixWorld = this.matrixWorld;
  const start = (this.userData[RAYCAST_START_KEY] as number | undefined) ?? 0;

  _rcProbe.geometry = this.geometry;
  _rcProbe.material = this.material;
  if (_rcProbe.material === undefined) {
    return;
  }

  if (this.boundingSphere === null) {
    this.computeBoundingSphere();
  }
  _rcSphere.copy(this.boundingSphere!);
  _rcSphere.applyMatrix4(matrixWorld);
  if (raycaster.ray.intersectsSphere(_rcSphere) === false) {
    return;
  }

  for (let instanceId = start; instanceId < this.count; instanceId++) {
    this.getMatrixAt(instanceId, _rcLocal);
    _rcWorld.multiplyMatrices(matrixWorld, _rcLocal);
    _rcProbe.matrixWorld = _rcWorld;
    _rcProbe.raycast(raycaster, _rcHits);

    for (let i = 0, l = _rcHits.length; i < l; i++) {
      const intersect = _rcHits[i];
      intersect.instanceId = instanceId;
      intersect.object = this;
      intersects.push(intersect);
    }
    _rcHits.length = 0;
  }
}

/**
 * Largest `i` with `layerStart[i] <= instanceId` — i.e. the layer an instance
 * belongs to. Binary search keeps hover O(log layers) instead of O(layers).
 */
function layerOfInstance(layerStart: Int32Array, instanceId: number): number {
  let lo = 0;
  let hi = layerStart.length - 2;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (layerStart[mid] <= instanceId) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }
  return lo;
}

/** Tag every cylinder mesh so a raycast `instanceId` resolves to a layer. */
export function tagInstanceRefs(model: GcodeModel): void {
  for (const rs of model.roleSegments) {
    if (!rs.mesh) {
      continue;
    }
    rs.mesh.raycast = raycastFromInstance;
    rs.mesh.userData[RAYCAST_START_KEY] = 0;
    rs.mesh.userData[GCODE_REF_KEY] = {
      roleSegments: rs,
      resolve: (instanceId: number): GcodeInstanceLocation => {
        const index = layerOfInstance(rs.layerStart, instanceId);
        const info = model.layers[index];
        return { layerIndex: index, z: info?.z ?? index, meta: info?.meta ?? EMPTY_LAYER_META };
      },
    } satisfies GcodeInstanceRef;
  }
}

/** Fallback metadata for an out-of-range hover (defensive; should not happen). */
const EMPTY_LAYER_META: LayerScalarMeta = {
  nozzleTemp: 0,
  tool: 0,
  layerTimeS: 0,
  fans: new Map<string, number>(),
};

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
  18: 'ironing',
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

/**
 * Low-detail bead: a four-sided **capped** tube, 16 triangles against the
 * octagon-plus-joint's 68.
 *
 * Two details matter and were both learned the hard way:
 *
 * - **Keep the default (diamond) orientation.** Rotating it 45° to get a
 *   flat-topped box looks more like a squished extrusion in isolation, but
 *   every bead on a layer then has a *horizontal* top face at exactly the same
 *   Z — and beads overlap constantly (at every path corner, and wherever the
 *   flow deliberately overlaps a neighbour). Coplanar faces at identical depth
 *   are textbook z-fighting, and it speckled the whole plate. The diamond puts
 *   a ridge at the top instead, so two overlapping beads differ in Z almost
 *   everywhere and the depth test stays decisive. The octagon used by the high
 *   LOD has the same ridge property, which is why it never showed the artifact.
 * - **Keep the caps.** An open tube shows its hollow interior wherever a path
 *   ends or turns sharply, which reads as beads being chopped off mid-air.
 *   Capping costs 8 triangles and removes that entirely.
 */
const segmentGeometryLow = new CylinderGeometry(0.5, 0.5, 1, 4, 1, false);
segmentGeometryLow.rotateX(Math.PI / 2); // Align along Z

/** Triangles per segment at each detail level (tube [+ joint ball]). */
export const TRIS_PER_SEGMENT_HIGH = 32 + 36;
export const TRIS_PER_SEGMENT_LOW = 16;

/** How much of the preview's geometry is drawn. */
export type GcodeDetail = 'high' | 'low';

// Seam dots are rendered as larger spheres.  We keep a dedicated geometry so
// they can be rendered independently of the normal joint spheres.
const seamDotGeometry = new SphereGeometry(0.5, 8, 6);

/**
 * Inject a per-instance opacity (`aOpacity`) multiply into a tube material
 * so individual extrusions can fade independently — Three.js `InstancedMesh`
 * has per-instance color but no per-instance alpha out of the box.
 *
 * The same hook carries the layer *lower* bound. Instances are packed
 * layer-ascending so the upper bound (and the progress scrub) is just a `count`
 * prefix, but a lower bound is not expressible as a prefix. Rather than split
 * the buffer back up per layer, each instance carries its layer index
 * (`aLayer`) and the shader collapses anything below `uLayerMin` to a
 * zero-area point. Switching "show all layers" ↔ "single layer" is then a
 * single uniform write instead of touching thousands of objects.
 */
function installInstanceShaderHooks(
  material: MeshPhongMaterial | LineBasicMaterial,
  layerMinUniform: { value: number },
  instanced: boolean,
): void {
  material.onBeforeCompile = (shader) => {
    shader.uniforms['uLayerMin'] = layerMinUniform;
    const opacityDecl = instanced ? 'attribute float aOpacity;\nvarying float vOpacity;\n' : '';
    const opacityAssign = instanced ? '\n  vOpacity = aOpacity;' : '';
    shader.vertexShader =
      `${opacityDecl}attribute float aLayer;\nuniform float uLayerMin;\n` +
      shader.vertexShader.replace(
        '#include <begin_vertex>',
        '#include <begin_vertex>' +
          opacityAssign +
          '\n  if (aLayer < uLayerMin) { transformed = vec3(0.0); }',
      );
    if (instanced) {
      shader.fragmentShader =
        'varying float vOpacity;\n' +
        shader.fragmentShader.replace(
          '#include <dithering_fragment>',
          '#include <dithering_fragment>\n  gl_FragColor.a *= vOpacity;',
        );
    }
  };
}

/** A zeroed per-role tally. */
function emptyRoleCounts(): Record<RoleName, number> {
  return {
    outerWall: 0,
    innerWall: 0,
    overhangPerimeter: 0,
    infill: 0,
    solidInfill: 0,
    gapFill: 0,
    bridge: 0,
    internalBridge: 0,
    topSurface: 0,
    ironing: 0,
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
}

/**
 * Build the whole preview: one instanced buffer pair per role, covering every
 * layer, packed layer-ascending.
 *
 * The packing order is the load-bearing part. Because every layer's segments
 * are contiguous and ordered, "show layers 0..N with the top one scrubbed to
 * fraction p" is exactly a *prefix* of each role's buffer — so the layer slider
 * and the nozzle scrub cost one `count` write per role instead of walking
 * thousands of per-layer objects.
 */
export function buildGcodeModel(
  source: GcodeLayerSource,
  colors: RoleColorPalette = ROLE_COLORS_DARK,
): GcodeModel {
  const group = new Group();
  const layerCount = source.layerCount();

  const layers: LayerInfo[] = [];
  const layerBlockData: Float32Array[][] = [];
  const layerBlockRoles: RoleName[][] = [];
  const roleTotals = emptyRoleCounts();
  const perLayerRoleCount = new Map<RoleName, Int32Array>();
  let totalSegments = 0;

  // Pass 1 — read every layer once. `blockData` copies out of WASM memory, so
  // the arrays are cached here rather than paying for a second crossing (and a
  // second copy of ~40 MB on a large plate) during the fill pass.
  for (let i = 0; i < layerCount; i++) {
    const buf = source.getLayer(i);
    const blocks = buf.blocksCount();
    const datas: Float32Array[] = [];
    const roles: RoleName[] = [];
    const blockLayout: { role: RoleName; count: number }[] = [];
    let layerSegments = 0;

    for (let b = 0; b < blocks; b++) {
      const data = buf.blockData(b);
      if (data.length === 0) {
        continue;
      }
      const count = data.length / FLOATS_PER_SEGMENT;
      const role = ROLE_ID_TO_NAME[buf.blockRole(b)] || 'other';

      datas.push(data);
      roles.push(role);
      blockLayout.push({ role, count });
      roleTotals[role] += count;
      layerSegments += count;

      let per = perLayerRoleCount.get(role);
      if (!per) {
        per = new Int32Array(layerCount);
        perLayerRoleCount.set(role, per);
      }
      per[i] += count;
    }

    layerBlockData.push(datas);
    layerBlockRoles.push(roles);
    layers.push({
      index: i,
      z: buf.z ?? i,
      totalSegments: layerSegments,
      blockLayout,
      meta: readLayerMeta(buf),
    });
    totalSegments += layerSegments;
  }

  // Pass 2 — allocate exactly one buffer pair per role for the entire plate.
  const roleSegmentsMap: Partial<Record<RoleName, RoleSegments>> = {};
  for (const role of ROLE_ORDER) {
    const count = roleTotals[role];
    if (count === 0) {
      continue;
    }

    const per = perLayerRoleCount.get(role) ?? new Int32Array(layerCount);
    const layerStart = new Int32Array(layerCount + 1);
    for (let i = 0; i < layerCount; i++) {
      layerStart[i + 1] = layerStart[i] + per[i];
    }

    const layerMinUniform = { value: 0 };
    const color = colors[role];

    if (role === 'travel') {
      const geometry = new BufferGeometry();
      geometry.setAttribute('position', new BufferAttribute(new Float32Array(count * 6), 3));
      // Two vertices per segment, so the layer tag is per-vertex here.
      geometry.setAttribute('aLayer', new BufferAttribute(new Float32Array(count * 2), 1));
      const material = new LineBasicMaterial({ color });
      installInstanceShaderHooks(material, layerMinUniform, false);
      const lines = new LineSegments(geometry, material);
      group.add(lines);
      roleSegmentsMap[role] = { role, lines, count, layerStart, layerMinUniform };
      continue;
    }

    if (role === 'seam') {
      // Seam points are rendered as spheres — no cylinder body, just dots.
      // A small specular keeps the marker dots reading as slightly glossier
      // than the matte tubes.
      const material = new MeshPhongMaterial({
        color,
        emissive: color,
        emissiveIntensity: EXTRUSION_EMISSIVE_INTENSITY,
        specular: 0x222222,
        shininess: 30,
      });
      installInstanceShaderHooks(material, layerMinUniform, true);
      const geometry = seamDotGeometry.clone();
      const jointsOpacity = new InstancedBufferAttribute(new Float32Array(count).fill(1), 1);
      jointsOpacity.setUsage(35044 /* THREE.DynamicDrawUsage */);
      geometry.setAttribute('aOpacity', jointsOpacity);
      geometry.setAttribute('aLayer', new InstancedBufferAttribute(new Float32Array(count), 1));
      const dots = new InstancedMesh(geometry, material, count);
      dots.instanceMatrix.setUsage(35044 /* THREE.DynamicDrawUsage */);
      dots.count = count;
      group.add(dots);
      // Re-use the `joints` slot so existing visibility / progress logic works.
      roleSegmentsMap[role] = {
        role,
        joints: dots,
        count,
        layerStart,
        layerMinUniform,
        jointsOpacity,
      };
      continue;
    }

    const material = new MeshPhongMaterial({
      color,
      emissive: color,
      emissiveIntensity: EXTRUSION_EMISSIVE_INTENSITY,
      specular: 0x000000,
    });
    installInstanceShaderHooks(material, layerMinUniform, true);

    // Per-mesh geometry clones so each carries its own per-instance attributes
    // (instanced attributes can't be shared across meshes). Both tube LODs are
    // built up front and share those attributes, so switching detail is a
    // geometry pointer swap — no instance data is touched and the instance
    // bounding sphere stays valid (both LODs have identical extents).
    const segGeom = segmentGeometry.clone();
    const segGeomLow = segmentGeometryLow.clone();
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
    segGeomLow.setAttribute('aOpacity', meshOpacity);
    jointGeom.setAttribute('aOpacity', jointsOpacity);

    const meshLayers = new InstancedBufferAttribute(new Float32Array(count), 1);
    const jointLayers = new InstancedBufferAttribute(new Float32Array(count), 1);
    segGeom.setAttribute('aLayer', meshLayers);
    segGeomLow.setAttribute('aLayer', meshLayers);
    jointGeom.setAttribute('aLayer', jointLayers);

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
      layerStart,
      layerMinUniform,
      meshGeomHigh: segGeom,
      meshGeomLow: segGeomLow,
      widths: new Float32Array(count),
      heights: new Float32Array(count),
      speeds: new Float32Array(count),
      accels: new Float32Array(count),
      meshOpacity,
      jointsOpacity,
    };
  }

  // Pass 3 — fill instances, walking layers in order so each role's buffer ends
  // up layer-ascending (the invariant `layerStart` and the `count` prefix rely on).
  const cursors = emptyRoleCounts();
  for (let i = 0; i < layerCount; i++) {
    const datas = layerBlockData[i];
    const roles = layerBlockRoles[i];

    for (let b = 0; b < datas.length; b++) {
      const data = datas[b];
      const role = roles[b];
      const rs = roleSegmentsMap[role];
      if (!rs) {
        continue;
      }

      const count = data.length / FLOATS_PER_SEGMENT;
      const baseOffset = cursors[role];

      if (role === 'travel') {
        const geometry = rs.lines!.geometry;
        const pts = (geometry.getAttribute('position') as BufferAttribute).array as Float32Array;
        const tags = (geometry.getAttribute('aLayer') as BufferAttribute).array as Float32Array;
        for (let k = 0; k < count; k++) {
          const off = k * FLOATS_PER_SEGMENT;
          const globalI = baseOffset + k;
          const pOff = globalI * 6;
          pts[pOff] = data[off];
          pts[pOff + 1] = data[off + 1];
          pts[pOff + 2] = data[off + 2];
          pts[pOff + 3] = data[off + 3];
          pts[pOff + 4] = data[off + 4];
          pts[pOff + 5] = data[off + 5];
          tags[globalI * 2] = i;
          tags[globalI * 2 + 1] = i;
        }
      } else if (role === 'seam') {
        // Seam blocks are degenerate (p0 == p1).  Render as a scaled white dot
        // sphere positioned at p0.  The width field (data[6]) carries the dot
        // radius set to SEAM_DOT_RADIUS (0.6 mm) in the Rust parser.
        const dots = rs.joints!;
        const tags = (dots.geometry.getAttribute('aLayer') as InstancedBufferAttribute)
          .array as Float32Array;
        for (let k = 0; k < count; k++) {
          const globalI = baseOffset + k;
          const offset = k * FLOATS_PER_SEGMENT;
          const dotSize = data[offset + 6] || 0.6; // 0.6 = SEAM_DOT_RADIUS in parser.rs
          _dummy.position.set(data[offset], data[offset + 1], data[offset + 2]);
          _dummy.rotation.set(0, 0, 0);
          _dummy.scale.set(dotSize, dotSize, dotSize);
          _dummy.updateMatrix();
          dots.setMatrixAt(globalI, _dummy.matrix);
          tags[globalI] = i;
        }
      } else {
        const mesh = rs.mesh!;
        const joints = rs.joints!;
        const meshTags = (mesh.geometry.getAttribute('aLayer') as InstancedBufferAttribute)
          .array as Float32Array;
        const jointTags = (joints.geometry.getAttribute('aLayer') as InstancedBufferAttribute)
          .array as Float32Array;

        for (let k = 0; k < count; k++) {
          const globalI = baseOffset + k;
          const offset = k * FLOATS_PER_SEGMENT;

          _p0.set(data[offset], data[offset + 1], data[offset + 2]);
          _p1.set(data[offset + 3], data[offset + 4], data[offset + 5]);
          const width = data[offset + 6] || 0.4;
          const height = data[offset + 7] || 0.2;
          if (rs.widths) rs.widths[globalI] = width;
          if (rs.heights) rs.heights[globalI] = height;
          if (rs.speeds) rs.speeds[globalI] = data[offset + SPEED_OFFSET];
          if (rs.accels) rs.accels[globalI] = data[offset + ACCEL_OFFSET];

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

          meshTags[globalI] = i;
          jointTags[globalI] = i;
        }
      }

      cursors[role] += count;
    }
  }

  const roleSegments: RoleSegments[] = Object.values(roleSegmentsMap) as RoleSegments[];
  for (const rs of roleSegments) {
    if (rs.mesh) {
      rs.mesh.instanceMatrix.needsUpdate = true;
      rs.mesh.geometry.getAttribute('aLayer').needsUpdate = true;
    }
    if (rs.joints) {
      rs.joints.instanceMatrix.needsUpdate = true;
      rs.joints.geometry.getAttribute('aLayer').needsUpdate = true;
    }
    if (rs.lines) {
      rs.lines.geometry.getAttribute('position').needsUpdate = true;
      rs.lines.geometry.getAttribute('aLayer').needsUpdate = true;
    }
  }

  return { group, layers, roleSegments, totalSegments, visibleSegments: totalSegments };
}

/** Release every Three.js resource owned by a built model. */
export function disposeGcodeModel(model: GcodeModel): void {
  for (const child of model.group.children) {
    if (child instanceof InstancedMesh || child instanceof LineSegments) {
      child.geometry.dispose();
      if (Array.isArray(child.material)) {
        for (const m of child.material) m.dispose();
      } else {
        child.material.dispose();
      }
    }
  }
  // The inactive tube LOD is not parented, so free it explicitly.
  for (const rs of model.roleSegments) {
    const inactive = rs.mesh?.geometry === rs.meshGeomHigh ? rs.meshGeomLow : rs.meshGeomHigh;
    inactive?.dispose();
  }
  model.group.clear();
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
  model: GcodeModel,
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

  for (const rs of model.roleSegments) {
    if (rs.role === 'travel') {
      if (rs.lines) (rs.lines.material as LineBasicMaterial).color.set(colors.travel);
      continue;
    }
    if (rs.role === 'seam') {
      if (rs.joints) {
        const m = rs.joints.material as MeshPhongMaterial;
        c.set(colors.seam);
        m.emissive.copy(c);
        m.color.copy(c).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
      }
      continue;
    }

    const { mesh, joints, widths, heights, speeds, accels, count } = rs;

    if (channel && channel.scope === 'segment' && mesh && widths && heights && speeds && accels) {
      // Per-instance color; keep the material white (and unlit emissive off)
      // so the per-instance scalar tint shows unmodulated.
      const mm = mesh.material as MeshPhongMaterial;
      mm.color.set(0xffffff);
      mm.emissive.setHex(0x000000);
      ensureInstanceColor(mesh, count);
      if (joints) {
        const jm = joints.material as MeshPhongMaterial;
        jm.color.set(0xffffff);
        jm.emissive.setHex(0x000000);
        ensureInstanceColor(joints, count);
      }
      const meshAlpha = rs.meshOpacity?.array as Float32Array | undefined;
      const jointAlpha = rs.jointsOpacity?.array as Float32Array | undefined;
      for (let i = 0; i < count; i++) {
        const value = channel.extract(widths[i], heights[i], speeds[i], accels[i]);
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
    } else if (channel && channel.scope === 'layer') {
      // A layer channel resolves to one value per layer. The role's material is
      // now shared by every layer, so the per-layer tint has to travel as
      // per-instance colour over each layer's slice of the buffer.
      const mm = mesh?.material as MeshPhongMaterial | undefined;
      if (mm) {
        mm.color.set(0xffffff);
        mm.emissive.setHex(0x000000);
      }
      if (mesh) ensureInstanceColor(mesh, count);
      if (joints) ensureInstanceColor(joints, count);
      const meshAlpha = rs.meshOpacity?.array as Float32Array | undefined;
      const jointAlpha = rs.jointsOpacity?.array as Float32Array | undefined;

      for (const info of model.layers) {
        const from = rs.layerStart[info.index];
        const to = rs.layerStart[info.index + 1];
        if (from === to) {
          continue;
        }
        const layerValue = channel.extractLayer(info.meta, fanKey);
        // No reading for this layer — fall back to the flat role colour.
        if (layerValue === null) {
          c.set(colors[rs.role]).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
        } else {
          const t = span > 0 ? (layerValue - range.min) / span : 0.5;
          c.set(sampleSpeedColor(t));
          if (outOfBand(layerValue)) c.multiplyScalar(OUT_OF_BAND_DIM);
        }
        const dim = layerValue !== null && outOfBand(layerValue);
        const alpha = bandActive && dim ? OUT_OF_BAND_ALPHA : 1;
        for (let i = from; i < to; i++) {
          if (mesh) mesh.setColorAt(i, c);
          if (joints) joints.setColorAt(i, c);
          if (meshAlpha) meshAlpha[i] = alpha;
          if (jointAlpha) jointAlpha[i] = alpha;
        }
      }
      if (mesh?.instanceColor) mesh.instanceColor.needsUpdate = true;
      if (joints?.instanceColor) joints.instanceColor.needsUpdate = true;
      if (rs.meshOpacity) rs.meshOpacity.needsUpdate = true;
      if (rs.jointsOpacity) rs.jointsOpacity.needsUpdate = true;
      applyMeshTransparency(rs, bandActive);
    } else {
      // Category: role color on the shared material, and neutralize any
      // leftover per-instance scalar tint / transparency.
      c.set(colors[rs.role]);
      if (mesh) {
        const m = mesh.material as MeshPhongMaterial;
        m.emissive.copy(c);
        m.color.copy(c).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
        resetInstanceColor(mesh);
      }
      if (joints) {
        const m = joints.material as MeshPhongMaterial;
        m.emissive.copy(c);
        m.color.copy(c).multiplyScalar(EXTRUSION_DIFFUSE_TINT);
        resetInstanceColor(joints);
      }
      fillOpacity(rs.meshOpacity, 1);
      fillOpacity(rs.jointsOpacity, 1);
      applyMeshTransparency(rs, false);
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
  const material = (rs.mesh?.material ?? rs.joints?.material) as MeshPhongMaterial | undefined;
  if (!material) return;
  material.depthWrite = !transparent;
  if (material.transparent !== transparent) {
    material.transparent = transparent;
  }
}

/**
 * Show layers `[min, max]` with the top layer scrubbed to `progress`.
 *
 * Both bounds collapse to cheap primitives thanks to the layer-ascending
 * packing:
 *
 * - the **upper** bound and the scrub are a prefix of each role's buffer, so
 *   they are a single `count` write (`InstancedMesh.count` / `setDrawRange`);
 * - the **lower** bound is only ever `0` (show everything up to `max`) or
 *   `max` (show that one layer), and rides the `uLayerMin` uniform, which the
 *   vertex shader uses to collapse instances below it.
 *
 * Neither path touches per-layer objects, so the cost is O(roles) — about 18
 * writes — regardless of how many layers the plate has.
 */
export function applyLayerVisibility(
  model: GcodeModel,
  min: number,
  max: number,
  progress: number,
): void {
  const layerCount = model.layers.length;
  if (layerCount === 0) {
    return;
  }
  const top = Math.max(0, Math.min(layerCount - 1, Math.round(max)));
  const info = model.layers[top];

  // How much of the top layer is revealed, split per role in path order.
  const visibleCounts = emptyRoleCounts();
  let remaining = Math.round(Math.max(0, Math.min(1, progress)) * info.totalSegments);
  for (const block of info.blockLayout) {
    const show = Math.min(remaining, block.count);
    visibleCounts[block.role] += show;
    remaining -= show;
    if (remaining <= 0) {
      break;
    }
  }

  const bottom = rs_clampLayer(min, layerCount);
  let visible = 0;
  for (const rs of model.roleSegments) {
    const shown = rs.layerStart[top] + (visibleCounts[rs.role] || 0);
    if (rs.mesh) {
      rs.mesh.count = shown;
      // Keep the hover raycast in step with the shader's lower bound.
      rs.mesh.userData[RAYCAST_START_KEY] = rs.layerStart[bottom];
    }
    if (rs.joints) rs.joints.count = shown;
    if (rs.lines) rs.lines.geometry.setDrawRange(0, shown * 2);
    rs.layerMinUniform.value = min;
    // Instances below `uLayerMin` are still submitted but collapsed in the
    // vertex shader, so they cost vertex work; count them out of the budget.
    visible += Math.max(0, shown - rs.layerStart[bottom]);
  }
  model.visibleSegments = visible;
}

/** Clamp a layer index into `[0, layerCount - 1]`. */
function rs_clampLayer(index: number, layerCount: number): number {
  return Math.max(0, Math.min(layerCount - 1, Math.round(index)));
}

export function applyHiddenRoles(model: GcodeModel, hiddenRoles: ReadonlySet<RoleName>): void {
  for (const rs of model.roleSegments) {
    const visible = !hiddenRoles.has(rs.role);
    if (rs.mesh) rs.mesh.visible = visible;
    if (rs.joints) rs.joints.visible = visible;
    if (rs.lines) rs.lines.visible = visible;
  }
}

/**
 * Select how much geometry the preview draws.
 *
 * `high` is the octagonal capped tube plus its corner joint ball (68 tris per
 * segment); `low` is the open box tube alone (8). Detail is a *pointer swap*
 * plus a visibility flag — no instance data is rebuilt — so it is safe to drive
 * from the camera every frame.
 *
 * Joints only ever fill the small wedge on the outside of a bend, so dropping
 * them is invisible until the bead is several pixels wide; the box tube loses
 * only the rounded silhouette.
 */
export function setDetailLevel(model: GcodeModel, detail: GcodeDetail): void {
  const high = detail === 'high';
  for (const rs of model.roleSegments) {
    if (rs.mesh && rs.meshGeomHigh && rs.meshGeomLow) {
      // Both LODs have identical extents, so the instance bounding sphere
      // computed for one stays correct for the other — deliberately not
      // invalidated here, since recomputing it walks every instance matrix.
      rs.mesh.geometry = high ? rs.meshGeomHigh : rs.meshGeomLow;
    }
    // Seam markers live in the `joints` slot but are meaningful dots, not
    // corner filler — they must keep their own role visibility.
    if (rs.role !== 'seam' && rs.joints) {
      rs.joints.visible = high;
    }
  }
}
