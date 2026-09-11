# Core — The Slicing Pipeline

This module turns a baked, world-space `Mesh` into a stack of `SliceLayer`s
ready for G-code emission. It is the engine's main loop.

> _Triangles in. Layers out. Order of operations matters._

---

## Why it exists

Slicing is not a single algorithm — it's seven of them, glued together in a
specific order, each consuming what the previous one produced:

1. Cut the mesh into 2D contours (one set per Z plane).
2. Replace those contours with variable-width wall beads (Arachne).
3. Snapshot the infill region _before_ any walls get stripped.
4. Strip inner walls from first-layer / top-surface islands when configured.
5. Detect and fill top / bottom solid surfaces (with bridge sub-classification).
6. Re-tag perimeters that cross unsupported air as `OverhangPerimeter`.
7. Add sparse infill to whatever is left.
8. Generate support structures under overhangs steeper than the threshold
   angle (`support_enabled`), tagged `ExtrusionRole::Support`. These read a
   perimeter snapshot taken back at step 5, before step 6 retags and splits
   the overhanging walls they would otherwise measure.
9. Order all paths per layer to minimise travel — including rotating closed
   loops to start at the configured seam vertex — so support strands are
   ordered alongside everything else.

Each step depends on the geometric output of the one before. Putting them in
the wrong order — or running surface detection on the original contours
instead of the post-Arachne ones — produces visibly wrong G-code. The
pipeline lives here, in [`pipeline::process_mesh`](pipeline.rs), as one
function so the order is impossible to misread.

---

## The contract

1. **`process_mesh` is the only public entry point** for the full pipeline —
   and [`slice_plate`](objects.rs) is the only way to reach *it*. The CLI, the
   WS server, the wasm preview and the desktop bridge all hand `slice_plate` a
   list of placed objects; it decides whether the plate can be merged into one
   mesh (the default) or has to be sliced object by object. There is no
   "subset" pipeline; partial slicing is achieved by feeding fewer params,
   not by skipping steps.
2. **All progress is reported through `ProcessLogger`.** No `eprintln!`,
   no `println!`. CLI verbosity and WS log streaming are then identical
   by construction.
3. **Order of operations is fixed.** The comments in
   [`pipeline.rs`](pipeline.rs) explain _why_ each step sits where it does;
   re-ordering is a behaviour change, not a refactor.
4. **`SliceLayer` is the sole carrier between phases.** Each phase reads from
   and writes back into the same `Vec<SliceLayer>`; nothing escapes to
   global state.
5. **Object identity is added around the pipeline, never inside it.** A part
   knows nothing of its neighbours: [`objects.rs`](objects.rs) slices each one
   with the untouched pipeline and only then tags and interleaves the results.
   No phase branches on "which object is this?".

---

## Anatomy

```mermaid
classDiagram
    class SliceLayer {
        +z: f64
        +paths: Paths
        +path_roles: Vec~ExtrusionRole~
        +path_widths: Vec~Option~f64~~
        +solid_regions: Paths
        +unsupported_regions: Paths
        +path_objects: Vec~Option~usize~~
    }
    class ExtrusionRole {
        <<enum>>
        OuterWall
        InnerWall
        OverhangPerimeter
        Infill
        Bridge
        TopSurface
        BottomSurface
        Support
        Skirt
    }
    SliceLayer "1" *-- "*" ExtrusionRole : path_roles
```

`paths`, `path_roles`, `path_widths` and `path_objects` are parallel arrays —
index `i` identifies the same emitted contour across all of them. `path_objects`
follows the `path_overhang` convention: **empty means "not sliced object-aware"**
and a `None` entry means "belongs to no object" (plate-wide bed adhesion). Any
helper that rebuilds these arrays has to carry every one of them, or the tags
shift onto the wrong paths. `solid_regions` is a
union of every top / bottom surface area on this layer; sparse infill
subtracts from it to avoid double-printing. `unsupported_regions` is the raw
layer footprint that has nothing solid in the layer below; the wall-
classification post-pass uses it to flag walls printed in air as
`OverhangPerimeter`.

---

## Pipeline order

```mermaid
flowchart TD
    M[Mesh<br/>baked, world-space] --> S1[slice_mesh<br/>raw OuterWall contours]
    S1 --> DC[apply_dimensional_compensation<br/>XY size · medial-limited elephant foot]
    DC --> S2[generate_arachne_walls<br/>variable-width beads]
    S2 --> S3{snapshot infill regions?}
    S3 -->|first/top single-wall enabled| SN[pre_strip_infill_regions]
    S3 -->|otherwise| W
    SN --> W[apply_single_wall_restrictions<br/>per-island]
    W --> I[interior_regions<br/>= calculate_interior_region per layer]
    I --> SF[generate_top_bottom_surfaces_with_interior<br/>3-stage bridge filter + PCA angle]
    SF --> OV[classify_overhang_perimeters<br/>walls in air → OverhangPerimeter]
    OV --> IN[add_infill_to_layers<br/>uses pre-strip regions − solid_regions]
    IN --> CB[combine_fill_areas<br/>infill_every_layers · solid_infill_every_layers]
    CB --> AN[connect_infill<br/>anchor line ends to the perimeter]
    AN --> SP[generate_supports<br/>overhang detection → projected columns<br/>reads the pristine perimeter snapshot]
    SP --> ORD[order_paths_per_layer<br/>greedy NN + seam vertex rotation<br/>skipped for monotonic surfaces]
    ORD --> L[Vec~SliceLayer~]
```

### Dimensional compensation

[`compensation.rs`](compensation.rs) corrects the raw contours **before**
anything is built from them, which is the only place the correction can be made
once. Walls, interior, surfaces and infill are all derived from these shapes, so
a contour corrected here corrects the whole layer; a *bead* corrected after the
fact would leave the interior — and therefore every solid surface — sized to the
uncorrected model.

Two corrections share the pass. `xy_size_compensation_mm` is a plain signed
offset on every layer, for a machine that prints consistently over- or
undersize. `elephant_foot_compensation_mm` shrinks the layers at the bed to undo
the squish, and is deliberately **not** a uniform offset — see the module docs
for why, and [§ Critical invariants](#5-elephant-foot-is-medial-limited-never-uniform).

The whole pass is skipped when neither is configured, which is the default, so
an ordinary slice never allocates for it — `ElephantFootConfig::resolve` returns
`None`, and the QA baselines therefore cannot move.

`apply_elephant_foot` is additionally **raft-gated and bed-gated**: skipped when
`adhesion_type = raft`, because the first layer then lands on sacrificial
material across an air gap; and skipped for any layer whose `z` is inconsistent
with resting on the plate, which is how a lifted or sequentially-printed object
avoids being "corrected" in mid-air.

### The first layer has its own height, set at both ends of the pipeline

`first_layer_height` is split deliberately. The slicer gives layer 0 its own Z
span — sampled at its **own mid-plane**, so an unset value reproduces the uniform
slicer plane-for-plane and the QA baselines cannot move — and
`mark_first_layer_height` then charges it through `path_heights` *after*
adhesion, so a skirt sharing that layer's Z is charged at the same height.

**Both ends resolve the value through `resolved_first_layer_height`**, which the
G-code generator also uses to decide which layer gets first-layer speeds and
temperatures. The two must never disagree about which layer is the first. It is
suppressed under a raft, which owns bed contact and prints at `layer_height`.

### Path ordering & seam placement

After every path on a layer has been classified, the pipeline runs a
role-grouped greedy-nearest-neighbour ordering pass to minimise travel
between extrusions. Closed loops are **cyclic** — picking vertex 0 as the
start point would force unnecessary travel to a fixed first point — so the
ordering pass picks the start vertex per loop using
`SlicingParams::seam_position`:

| Policy                | Vertex choice                                                                                           | Use case                                   |
| --------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| `Nearest` _(default)_ | Vertex closest to the previous path's end                                                               | Fastest print, scattered seams             |
| `Rear`                | Vertex with the largest Y (tie-break smallest X)                                                        | Single back-of-model seam line             |
| `Aligned`             | Vertex with the largest projection onto a fixed direction (+Y)                                          | Consistent seam line across all layers     |
| `SharpestCorner`      | Vertex with the largest convex turn angle; falls back to `Nearest` for smooth loops (max corner < ~10°) | Hides the blob in geometry corners         |
| `Random`              | Hash of the loop's first-vertex bits (deterministic per loop)                                           | Spreads blobs evenly on cylinders/organics |

Once the start vertex is chosen the loop's vertices are rotated in place
(`pts[(start + k) % n]`) so the emitted G-code begins at that point. The
rotation is the only mutation — geometry is preserved exactly. Combined
with the role-grouped nearest-neighbour pass this single change reduced
benchmark travel by ~18 % vs always starting at vertex 0.

### Bridge & overhang quality

Bridge detection lives inside `generate_top_bottom_surfaces_with_interior`
and runs a three-stage filter on the raw "footprint with no support below"
region (matching OrcaSlicer / PrusaSlicer behaviour):

1. **Morphological opening** (`bridge_noise_filter_mm`, default 0.05 mm) —
   erode then dilate to wipe out sub-pixel slivers caused by Clipper2's
   Centi quantisation. Kept small so genuine 0.4 mm-wide bridge frames
   (window mullions, text overhangs) survive intact.
2. **Minimum-area filter** (`bridge_min_area_mm2`, default 0.5 mm²) — drop
   surviving islands smaller than the threshold; they get reclassified as
   ordinary `BottomSurface` so the layer remains fully solid below the gap.
3. **Anchor expansion** (`bridge_anchor_mm`, default 0.5 mm) — dilate the
   surviving regions outward and clip back to **`interior_regions[i]`**
   (the inside-walls bound) so each strand bites into the supported solid
   bottom material on either side of the gap _without_ expanding into the
   wall band.

Bridge **direction** uses principal-axis analysis (PCA) of the unsupported
region. Strands print perpendicular to the dominant axis so the shortest
possible span is bridged, even when the gap is rotated relative to the
print bed. Falls back to bounding-box short-axis when the region is
square / circular.

After surfaces are assigned, `classify_overhang_perimeters` re-tags each
`OuterWall` / `InnerWall` whose centerline is ≥ 50 % **inside or on the
boundary of** the layer's `unsupported_regions` as `OverhangPerimeter`.

The crucial detail is how `unsupported_regions` is built. Naïvely you
would think `perimeters[i] − perimeters[i-1]` (raw centerline difference),
but that is **wrong in two complementary ways** that took several rounds
to disentangle:

1. **`perimeters[i]` IS the wall path.** `perimeter_paths_of(layer)`
   returns the OuterWall centerline polygons of the layer — i.e. the same
   closed paths the wall classifier iterates over. So every wall vertex
   lies _exactly on_ the boundary of `perimeters[i]`, and therefore on the
   outer boundary of any region derived by subtracting another polygon
   set from `perimeters[i]`. A "strictly inside" parity test (treating
   `IsOn` as outside) flags **nothing**, ever.
2. **A current-layer wall is supported by the previous-layer bead, not
   by its centerline.** The previous-layer bead extends `d/2` outward
   from its centerline, so the geometric support envelope is
   `inflate(perimeters[i-1], +d/2)`.

The two fixes go together:

```text
unsupported_regions = perimeters[i] − inflate(perimeters[i-1], +d/2)
```

with `IsOn` counted as **inside** in the parity test:

- For a slight outward lean (horizontal step `S < d/2`) the inflated
  previous perimeter fully contains `perimeters[i]`, so
  `unsupported_regions` is empty → no wall flagged. This kills the
  "80 % of the Benchy is overhang" false positive without any vertex-
  fraction tuning.
- For a real overhang (`S > d/2`, ≈ 45° lean for 0.2 mm layer / 0.4 mm
  nozzle) a meaningful air strip exists. Wall vertices lie on its outer
  boundary (= the current centerline), and the `IsOn`-counts-as-inside
  parity test flags them.

**Don't** restore the raw centerline difference, the strict-inside
boundary policy, or the `0.6 × nozzle_diameter` "safety" erosion that
was tried at one point — any one of them suppresses _all_ overhang
detection. See `test_classify_overhang_e2e_*` in `walls.rs` for the
production-geometry lockdown tests.

Reclassified paths inherit the bridge speed (`bridge_speed`) and the fused-flow
width (`nozzle_diameter_mm × bridge_flow_ratio`, > 1× by default) in the G-code
generator and trigger the bridge fan boost via `has_bridges`. This eliminates
sagging walls printed across windows, slots, and similar mid-air features.

### Dynamic overhang speed & cooling

When `enable_overhang_speed` is set, the same pass grades each wall segment by
**overhang degree** — how far past the previous layer's material footprint its
centreline sits — and records an `OverhangClass` (`None`/`Deg1`…`Deg4`) per path.
The degrees are nested inflations of the previous perimeter (`prev`, `+d/4`, the
existing `d/2` air boundary, `+3d/4`), so `Deg3`/`Deg4` coincide exactly with the
binary `OverhangPerimeter` region and the role tag stays consistent.

Two invariants:

- **Off ⇒ byte-identical.** With grading off the classifier reduces to the
  historical air/support split and `path_overhang` stays empty.
- **Split only where output changes.** `overhang_band_class` folds any degree
  whose speed and fan match a plainer wall down to that class, so a wall is never
  fragmented into arcs that print identically — no wasted retracts.

Grading needs the **pristine** previous-layer perimeter, snapshotted before
bridge clipping splits any walls and passed in as `OverhangGrading`.

Two things to note:

- **Snapshot before strip.** The single-wall-strip step (step 4) removes
  walls _per island_, but `calculate_interior_region` would still
  miscount islands on the same layer if the strip happened first.
  Snapshotting the infill region while all walls are present prevents the
  sparse-infill boundary from ballooning into the wall zone on unaffected
  islands.
- **Surfaces use post-Arachne geometry.** Top/bottom detection compares
  `OuterWall` paths between adjacent layers. Running it before Arachne
  would compare raw mesh contours instead — same shape, but with
  inconsistent winding from the slicer.

---

## Phase catalog

| Phase                         | Function                                                                      | Reads                                       | Writes                                       |
| ----------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------- | -------------------------------------------- |
| Slice                         | [`slice_mesh`](slicer.rs)                                                     | `Mesh`                                      | `paths` (OuterWall)                          |
| Dimensional compensation      | [`compensation::apply_dimensional_compensation`](compensation.rs)             | `paths` (raw contours), layer `i + 1`       | `paths` (corrected; skipped when unconfigured) |
| Arachne walls                 | [`walls::arachne::generate_arachne_walls`](../walls/arachne/mod.rs)                        | `paths`                                     | `paths`, `path_roles`, `path_widths`         |
| Infill snapshot               | [`infill::calculate_interior_region`](infill.rs)                              | `paths` (all walls)                         | `pre_strip_infill_regions` local             |
| Single-wall strip             | [`walls::apply_single_wall_restrictions`](walls.rs)                           | `paths`, `path_roles`                       | `paths`, `path_roles` (inner walls + first-layer gap fill removed) |
| Interior regions for surfaces | [`infill::calculate_interior_region`](infill.rs)                              | `paths` (post-strip)                        | `interior_regions` local                     |
| Top / bottom surfaces         | [`surfaces::generate_top_bottom_surfaces_with_interior`](surfaces.rs)         | `paths`, `interior_regions`                 | `paths`, `path_roles`, `solid_regions`       |
| Overhang classification       | [`walls::classify_overhang_perimeters`](walls.rs)                             | `paths`, `unsupported_regions`, `OverhangGrading` (opt) | `path_roles` (some `OverhangPerimeter`), `path_overhang` (when grading) |
| Sparse infill                 | [`infill::add_infill_to_layers`](infill.rs)                                   | `pre_strip_infill_regions`, `solid_regions` | `paths`, `path_roles`, `path_heights`        |
| Path ordering & seams         | inline in [`pipeline::process_mesh`](pipeline.rs) (uses `choose_seam_vertex`) | `paths`, `path_roles`, `seam_position`      | `paths` (rotated/reordered)                  |

`pre_strip_infill_regions` is computed only when at least one of
`only_one_wall_first_layer` / `only_one_wall_top` is enabled; otherwise the
post-strip and pre-strip regions are identical and the snapshot would be
wasted work.

---

## Role in the wider system

```mermaid
flowchart LR
    subgraph Inputs
        M[Mesh<br/>from scene::transform::apply_transform]
        P[SlicingParams<br/>from settings::]
    end
    M & P --> C[core::process_mesh]
    C --> L[Vec~SliceLayer~]
    L --> G[gcode::generate_gcode]
    G --> O[(.gcode file)]
```

The pipeline does not load files, does not write G-code, and does not know
about printer profiles. It is a pure function from `(Mesh, SlicingParams)`
to `Vec<SliceLayer>`, with logging as a side channel.

---

## Performance

### Native / server (x86-64, AArch64)

The three most expensive pipeline phases all parallelise across layers via
[`rayon`](https://docs.rs/rayon):

| Phase                     | Strategy                               | Notes                                                                                  |
| ------------------------- | -------------------------------------- | -------------------------------------------------------------------------------------- |
| `perimeter_snapshot`      | `par_iter().map` across all layers     | `perimeter_paths_of` runs concurrently per layer                                       |
| `surface_blocked`         | `into_par_iter().map` **gated**        | Wall-bead footprint built only for layers that produced a top/bottom surface region    |
| `surfaces` detection      | `into_par_iter().map(detect_region)`   | Each layer's bridge/top/bottom regions are independent                                 |
| `overhang_classification` | `par_iter().map(process_layer)`        | Each layer's densification + boundary tests are independent                            |

Measured on a 3DBenchy at 0.2 mm layer height (240 layers), 8-core host:

| Phase                     | Serial (before) | Parallel (after) | Δ        |
| ------------------------- | --------------- | ---------------- | -------- |
| `perimeter_snapshot`      | 3 004 ms        | 108 ms           | −96%     |
| `surfaces`                | 2 926 ms        | 273 ms           | −90%     |
| `overhang_classification` | 426 ms          | 54 ms            | −87%     |
| **Wall-clock total**      | **4 119 ms**    | **1 163 ms**     | **−72%** |

`compute_wall_bead_footprint` was also rewritten from one Clipper2
`inflate+union` _per wall path_ (quadratic accumulation) to one batched
`inflate` _per `(is_open, radius)` bucket_, typically reducing it to 1–2
Clipper2 calls per layer regardless of wall count.

Even batched, the wall-bead footprint is the single most expensive per-layer
artifact of the surface phase, so `surface_blocked` (the eroded-footprint ∪
gap-fill region the serial apply pass subtracts from the solid fill) is built
**only for layers that actually produced a top/bottom surface region**. The
gate runs after the detection pass, when each layer's `(bottom, top)` regions
are known; a layer with no surface never reads its `surface_blocked` entry, so
the empty placeholder left there is output-identical while skipping the
footprint construction for the majority of mid-model layers. On a 3DBenchy this
roughly halves the surface phase.

### WASM (`wasm32-unknown-unknown`)

`rayon` is excluded from the WASM build via `cfg(not(target_arch = "wasm32"))`.
The same phases fall back to sequential `iter()` / `map()`. Three additional
WASM-specific constraints reduce throughput further:

1. **Single-threaded execution.** WebAssembly in browsers runs on one JS
   thread. `SharedArrayBuffer`-based threading exists but requires
   cross-origin isolation headers that most hosts don't enable.
2. **Clipper2 C++ allocator shim.** `src/cpp_shims.rs` provides `operator
new/delete` implementations that compile Clipper2 entirely into the WASM
   binary — no `env` module imports, no JS boundary crossings. However,
   every C++ heap allocation prepends an 8-byte bookkeeping header, adding
   a small constant overhead to the many short-lived `Paths` objects that
   inflate/union produce in per-path loops.
3. **Memory pressure.** Wasm32 has a 4 GiB address limit and is typically
   allocated 256 MiB by browsers. Very large models or high layer counts
   can OOM before completion.

As a rough guide, expect the WASM slicer to be **5–15× slower** than the
native server-side slicer for the same model and settings. The browser UI
uses the WASM path only for the live preview; the production slice
(server or desktop) always runs the native binary.

**What this means in practice:**

| Scenario                  | Binary                  | Expected slice time (Benchy 0.2 mm) |
| ------------------------- | ----------------------- | ----------------------------------- |
| WS server / Tauri desktop | `slicer-engine` release | ~1 s                                |
| Browser preview (WASM)    | `scene_wasm.js`         | ~10–15 s                            |
| CI (cross-compile test)   | debug build             | ~10–20 s (LTO off)                  |

Per-phase timings are reported through the `ProcessLogger::log_phase_*`
hooks. Sub-timings for Arachne (`collapse_depth_ms`, `bead_shrink_ms`) and
surface generation (`perimeter_snapshot_ms`, `detection_ms`,
`infill_gen_ms`) are summed across worker threads on native, so they can
exceed the wall-clock duration of those phases.

---

## Object identity through slicing

A *plate* is several placed objects; layers are flat. Turning one into the other
without losing track of which part is which is what
[`objects.rs`](objects.rs) is for, and both exclude-object (cancel a part
mid-print) and sequential printing (finish one part before starting the next)
need exactly the same segmentation — so it is built once.

**[`slice_plate`](objects.rs) is the single slicing entry point.** CLI, WS
server, wasm `web-slicer` and the desktop bridge all hand it a `&[ObjectInput]`
instead of merging meshes themselves. Do not re-introduce a "just concatenate
the faces and call `process_mesh`" site — that merge is what erased object
identity in the first place.

### The merged fast path is not optional

When [`SlicingParams::object_aware()`](../settings/params.rs) is false — neither
`exclude_object` nor `print_sequence = by_object` — `slice_plate` merges and
calls `process_mesh` exactly as before, so the default configuration produces
**byte-identical G-code**. `merged_path_matches_a_plain_process_mesh` pins this.

Object-aware slicing runs the pipeline once *per object*, which is **not**
output-equivalent: `calculate_interior_region` averages the wall-bead count
across a layer's islands, so an island's interior estimate depends on what else
shares its layer. Slicing a part alone is the more faithful result, but it is
still a change, and must only happen when asked for.

### Rules that hold the segmentation together

- **`SliceLayer::path_objects` is a parallel array with an empty sentinel**,
  like `path_overhang`: empty means "not sliced object-aware", and `None` at an
  index means "belongs to no object". Every helper that rebuilds a layer's
  parallel arrays — notably [`adhesion::prepend`](../adhesion/mod.rs) — must
  carry it along, or the tags silently shift onto the wrong paths.
- **Adhesion is plate-wide in `by_layer`, object-owned in `by_object`.** In
  layer order the skirt or brim is generated once on the merged stack and tagged
  `None`, so cancelling one part does not take the plate's adhesion with it. In
  object order each object is a self-contained print and owns its own.
- **Layers merge by Z slot, not by index.** Two parts resting on the bed slice
  onto the same grid, but a part lifted off the bed keeps its own — its bottom
  layers are *its* bottom layers. `merge_layers_by_z` groups within a quarter
  layer and takes at most one layer per object per slot, so emitted Z is always
  strictly ascending.
- **Sequential order is front-to-back** (`min_y`, then `min_x`). The gantry
  sweeps from behind, so finishing the nearest part first keeps the carriage away
  from finished work longest. Clearance problems are **warnings, not errors** —
  the clearances are machine estimates, and refusing to slice would be worse than
  saying what to check. Only objects printed *before* another are height-checked;
  the last one has nothing reaching over it.
- **Object names are sanitised and de-duplicated here.** Klipper parses
  `EXCLUDE_OBJECT_DEFINE NAME=…` as a G-code parameter, so a space splits the
  token, and two parts sharing a name would cancel together. Every runtime feeds
  user-chosen filenames straight in, so `unique_object_name` fixes both centrally
  rather than at each call site.

Where the markers themselves are emitted — `M486` by default, `EXCLUDE_OBJECT_*`
for Klipper — is [`gcode`](../gcode/README.md)'s business.

### What belongs on the printer, not the process

Whether the machine *can* cancel an object (`exclude_object`) and how much room
its printhead needs (`extruder_clearance_height_mm` /
`extruder_clearance_radius_mm`) are properties of the **machine**, so all three
carry the **Hardware** `x-group`. Two printers can run the same `by_object`
process yet differ in gantry height, duct radius and firmware support. Only the
print-behaviour choices (`print_sequence`, `between_objects_gcode`) are process
settings. The clearances carry **no** `x-relevant-when` gate — a printer always
has a clearance, and gating it would point across contracts at a process field
in another tab.

---

## Support structure generation

[`supports.rs`](supports.rs) runs **after infill and before path ordering**. It
reads only `OuterWall` paths (via `perimeter_paths_of`) to derive each layer's
model footprint, so it never disturbs wall, surface or infill geometry, and
appends `ExtrusionRole::Support` **open** polylines. Running before ordering is
what gets support strands ordered and flow-compensated with the rest of the
layer.

### Detecting an overhang

```
overhang[i] = footprint[i] − inflate(footprint[i−1], max_step)
max_step    = layer_height · tan(threshold_from_vertical) + facet_tol
```

`support_threshold_angle` is measured **from vertical** — 45° is the classic 45°
rule, and a *smaller* angle triggers support on gentler overhangs. A facet
tolerance plus a `SUPPORT_MIN_OVERHANG_AREA_MM2` filter reject the slicing noise
a near-vertical faceted wall produces. Fill rule is **NonZero** throughout:
footprints are Clipper2-normalised frames with CW holes, and `Positive` would
erase their interiors.

Two things about that input are load-bearing:

- **Footprints come from a pristine snapshot, never from the live layer.**
  `classify_overhang_perimeters` retags an overhanging wall as
  `OverhangPerimeter` and splits its loop, so on a steep slope there is **no
  `OuterWall` path left** by the time supports run. A 60° cone reported 49 of 50
  footprints empty and got no support at any threshold — while every hand-built
  unit test passed. `process_mesh` therefore calls `snapshot_perimeters` before
  surface generation and hands the result to `generate_supports`, exactly as
  overhang grading already did.

  **Any test for support behaviour must go through `process_mesh`**
  ([tests/support_slice.rs](../../tests/support_slice.rs)); a unit test that
  builds `SliceLayer`s by hand cannot see this class of bug.
- **Per-layer contacts are welded before use.** A contact is only the *newly*
  exposed sliver at its layer, so down a continuous slope successive contacts are
  concentric rings separated by exactly `max_step` — 94 sub-paths ≈0.1 mm wide on
  a 60° frustum, which the fill scanline discards. `accumulate_support_area`
  closes the accumulation by just over half that gap, fusing them into the solid
  annulus between the model and the widest overhang above. The close only bridges
  *between* rings, so the supported area is unchanged — only its connectivity.

### Getting it to the bed

Each overhang is registered at its top-contact (activation) layer
`i − 1 − support_z_gap_layers`, leaving a Z air-gap for clean removal, then
accumulated top-down. The carried column is subtracted by
`inflate(footprint[i], support_xy_distance_mm + ½ outer-wall width)`.

**The half bead matters**: footprints are wall *centrelines*, so inflating by the
raw distance leaves only `xy − ½d` of real air — 0.6 mm of a requested 0.8 mm at
defaults.

- **Interface layers.** The top contact under an overhang and the bottom contact
  resting on the model, within `support_interface_layers`, are filled at the
  denser `support_interface_density`; the body uses `support_density`.
- **`support_on_build_plate_only`** keeps only what can descend to the bed
  through empty space. `covered[i]` accumulates the model footprint **strictly
  below** layer `i`, grown by `support_xy_distance_mm`, and contact pads
  overlapping it are dropped — so the overhang above prints unsupported.

  **Grow it by the same XY clearance the descent uses**, or a pad that clears the
  model by less than that survives the test and is then eaten away layer by
  layer, leaving a floating stub instead of a column. The mask is also subtracted
  from both column builders, so "no support ever rests on the model" holds by
  construction, and **tree re-checks it on every migration step** — a straight
  column cannot wander, but a tree tip moves in XY and a plate-reachable seed
  will otherwise drift over the print. The vector is built only when the option
  is on.

### Normal vs tree

| Type | Shape | Cost |
| --- | --- | --- |
| `Normal` | Carries the full overhang footprint down as a grid column | Benchy ≈17 k mm |
| `Tree` | Node-drop simulation — contact tips migrate toward their local centroid each layer, merge when they meet, and reject any step that would enter the model | Benchy ≈3.7 k mm |

Wide interface caps still cover the full overhang, so trunks stay thin; edge tips
lean inward and a wide field contracts into a few trunks (a wide flat plate costs
≈3× less than normal).

**Tree is a pragmatic approximation, not a full collision-avoiding branching tree
with base flaring** — the honest limitation is surfaced by
`unsupported_feature_warnings()`. Validate the converging shape with an XZ
(front-elevation) projection of the `Support material` beads.

### Widths, contours, and the raft

`ExtrusionRole::Support` already emits `;TYPE:Support material`. Each island is
drawn as a **closed contour plus open fill strands**, and carries **no** explicit
width — an explicit width would short-circuit `resolve_width_mm`'s fill-role
branch, which is what charges a support line the volume of the strip it fills
(`support_line_width` → `extrusion_flow_spacing_mm`) rather than a full nominal
bead. The contour is what keeps a thin column from degenerating into
disconnected dashes; runs below `2 × nozzle` are dropped, the same splat rule gap
fill uses.

- **Whether a path is drawn closed is `ExtrusionRole::forms_closed_loops`, one
  definition shared by the G-code generator and the path orderer.** The two
  disagreeing — the generator's closed-loop list omitted `Support`, the
  orderer's omitted nothing — silently dropped the segment that closes every
  support island back to its start, about a fifth of all support contour length
  on a test overhang.
- **The raft carries support columns, and only the raft's own bead width.**
  `printed_footprint` unions the object with the `Support` role's footprint so
  neither the raft nor the skirt starts a column in mid-air. The raft shares the
  role but stamps a deliberately coarser, explicit bead to match its own wider
  line pitch — `support_line_width` is for the fill-role branch above and must
  never reach it as a second override beneath.
- **Spiral (vase) mode forces `support_enabled` off** in
  `spiral_vase_normalized`. A vase is one continuous wall with retraction
  disabled; there is no discrete layer for a column to stand on and no way to
  travel to one.
- **`process_mesh_debug` generates supports too**, on the same pristine snapshot,
  recorded under `DebugStage::Support`. Skipping it left the QA gallery and
  `--debug-geometry` showing a support-enabled model with no support in the
  picture.

### Supports are generated *after* bridge classification — deliberately

`generate_supports` runs late, so bridge detection never learns that an overhang
is supported. With the default `support_z_gap_layers ≥ 1` that is **correct**:
the gap is real air, so the first model layer above support genuinely bridges and
wants bridge speed and full cooling. Measured on a cap-on-post model, support
stops at Z 9.70 and the cap's first layer at Z 10.10 spans a 0.4 mm void.

At `support_z_gap_layers = 0` the support does touch the overhang, and that layer
is still classified `Bridge`. Feeding support back into bridge *detection* is the
only way to change that, and perturbing that pass is warned against throughout
this document. **The asymmetry decides it**: bridge settings over supported
material print a slightly worse surface, while normal settings over real air
*fail*. So the conservative classification stands, the setting's own copy says
so, and nothing silently depends on it.

**If you do ever reorder it**, note that supports no longer need to run late for
their own sake — they read a pristine perimeter snapshot taken before wall
splitting, not the mutated layer. The only remaining reason for the current
position is that support strands are ordered by the TSP with the rest of the
layer.

---

## The infill / surface boundary

Everything that is not a wall is placed inside an **interior region**: the gross
island outline deflated by the walls that will sit on it.
[`calculate_interior_region`](infill.rs) computes it from the `OuterWall` paths,
winding preserved, deflated inward by

```
total_inward = (walls_per_island − 0.5) × nozzle_diameter − overlap_distance
```

The `−0.5 × d` accounts for `OuterWall` centrelines already being inset half a
bead from the model surface; without it the interior is over-shrunk by half a
bead width.

### The estimate is an average, and Arachne breaks the assumption

`walls_per_island = ceil(total_wall_bead_count / outer_contour_count)`. That is
only a mean, and Arachne places a *variable* number of variable-width beads per
island — even along one island. On a layer whose islands differ in bead count the
estimate is too low and the interior under-deflated by up to a full bead, so
solid surface, sparse infill and bottom fill land **on top of the innermost
wall**. The classic generator, which places a fixed count, shows none of it.

Three corrections follow from that one flaw, and each is deliberately narrow:

| Correction | Applied to | Why it is safe |
| --- | --- | --- |
| **Wall-footprint clip** | sparse infill · solid surfaces | Count- and width-agnostic, so it is a no-op wherever the estimate was already right |
| **Opened interior** | top/bottom surfaces · bridge candidates | Erases sub-bead *channels*; a real surface sits on a thick interior and keeps its full extent |
| **Sliver opening** | surface fill regions | A strip narrower than one bead cannot hold a bead by construction |

**None of them reshape `interior_regions` itself.** Bridge *detection* keys off
the smooth interior; reshaping it spawns phantom bridges from the jagged
bead-following boundary. Only the fill regions and the bridge *candidate* are
clipped.

### The wall-footprint clip

[`compute_wall_bead_footprint`](surfaces.rs) inflates every wall centreline by
its own half-width, giving the **actual** physical footprint.

- **Sparse infill** subtracts it grown by `infill_perimeter_gap_mm`, keeping its
  intended clearance from the real innermost wall.
- **Solid surfaces** subtract it eroded by `infill_overlap_percent × d`, so the
  fill still welds that much into the innermost wall — the designed bond — and no
  further. The un-eroded gap-fill footprint is unioned back in so surfaces
  *abut* gap fill rather than welding to it.

Use **`FillRule::NonZero`** for these subtractions, never `Positive`: the wall
footprint is a frame with CW hole sub-paths. `Positive` ignores CW holes, treats
the frame as a solid block, and erases the whole interior.

### Keeping tiny extrusions out of sparse infill

Three filters, each keyed to a different artifact. What they have in common is
that an isolated dab of material still costs a full retract → travel →
un-retract to reach, which is the waste being removed.

- **Grow `solid_regions` by one bead before subtracting it.** A surface is
  printed as a rectilinear serpentine whose *stepped* extent only approximates
  its nominal polygon, and the surface pass has already trimmed it off the wall
  band. Subtracting the raw outline leaves a crescent sliver along every curved
  perimeter, which the scanline shatters into sub-millimetre dashes — 31 on one
  3DBenchy layer, ~4.8 mm³ of filament pushed back and forth to deposit
  ~0.04 mm³. `SOLID_MARGIN_NOZZLE_MULT × nozzle` fixes it; `0.5` does not,
  because the sliver is wider than half a bead.

  **Key this to `solid_regions`, never to the infill area as a whole.** That
  makes it an exact no-op on layers with no solid surface, so a genuinely thin
  wall-to-wall cavity keeps its full lattice. An earlier attempt morphologically
  *opened* the whole infill area instead and could not tell an artifact sliver
  from a real thin cavity: it erased the filament caddy's hollow-box lattice
  outright (wall-zone void 62 → 146 mm², 35 % of its infill gone) and the quality
  gate caught it. A sweep found no safe threshold for that approach.
  `test_thin_cavity_without_solid_surface_keeps_its_infill` pins the right
  behaviour.
- **Skip a connected region too small to hold more than one dash**
  (`INFILL_MIN_REGION_AREA_NOZZLE_MULT × d²`, 2.0 mm² at 0.4 mm). Two properties
  make it safe. It is an **area rule on whole connected regions, never a width
  rule** — a cavity deserving a lattice is a *large* region that merely happens
  to be narrow, and across the QA corpus the caddy has no infill region at all
  between 0.01 mm² and 10 mm², an empty band two orders of magnitude wide. And
  it filters the **generated paths, not the region**: `generate_rectilinear_infill`
  seeds its scanline phase from the bounding box of the whole area, so deleting
  an outlying sliver *first* shifts every infill line on the layer. Membership is
  tested with segment **midpoints** — an infill line's endpoints lie exactly on
  the boundary, where the integer-scaled point-in-polygon test can land either
  side.
- **`min_infill_extrusion_mm`** still guards the residual sub-threshold segments
  a legitimate region's tapering corners produce.

### Thin wall-band channels, and phantom surfaces

Wherever a cross-section is *locally* thinner than the per-island average, the
interior estimate leaves a sliver channel (≤ ~1 mm): Benchy hull-side wall tips,
funnel-to-roof transitions, the cabin roof ridge, embossed calibration-cube
logos. Arachne already fills those solid with wall and gap-fill beads — but where
the geometry above or below recedes, the channel is detected as an "exposed"
surface and filled with a zig-zag of sub-millimetre segments. `classic`, whose
uniform offsets consume the same cross-sections, emits nothing there.

`open_interior_for_surface` erodes then dilates the interior by
`SURFACE_MIN_INTERIOR_WIDTH_NOZZLE_MULT × nozzle / 2`, erasing channels under
1.0 mm at a 0.4 mm nozzle. A surface landing entirely inside a thin channel
disappears; a genuine surface keeps its full extent, with only its corners
rounded.

- **The discriminator is the *interior*, not the strip.** A real fore-deck top
  surface is an equally thin band. What separates it from an artifact is that it
  sits on a *thick* interior. Filtering the strip by its own width wrongly
  deletes legitimate thin surfaces — do not do that.
- **Dropped strips stay solid** via the beads already filling them, and — no
  longer being a `solid_region` — their gap-fill beads survive
  `prune_redundant_gap_fill`. Expect a small **gap fill ↑ / surfaces ↓** shift in
  the QA baselines.
- **The absolute base cap (`i < bottom_layers`) is exempt** for bottom surfaces:
  that is bed contact and must stay fully solid for adhesion.
- **Bridges get the same clip**, in `clip_to_void` step A. The same
  under-deflation that spawns a phantom surface fires a phantom **bridge** in the
  same channel — laying sparse lines straight over the beads that already fill
  it. A genuine bridge over a wide void sits on a thick interior and is
  untouched. Only the candidate is clipped; bridge *detection* input never is.

### Sub-bead slivers at grazing angles

The wall-band trim subtracts a footprint whose boundary does not follow the
surface outline exactly. Where the two meet at a **grazing angle** the difference
is a long crescent far narrower than one bead — and because the fill direction is
then near-parallel to it, **every span is a stub**.

Measured on the caddy's hexagon logo: sliver sub-paths of ≈4.5 mm² at ≈0.22 mm
mean width along the two edges lying 15° off the fill direction, producing a
repeating 0.82 mm line / 0.62 mm connector micro-serpentine. **93 % of that
material was already covered** by the flanking bead or the normal surface.

`open_surface_region_for_fill` removes them:

- **The threshold is physical, not heuristic** — erode by
  `SURFACE_FILL_MIN_WIDTH_FRACTION (0.5) × solid-surface extrusion width`, an
  erosion *diameter* of exactly one bead.
- **It is a width filter, not an area filter.** Small-but-printable surfaces
  survive intact (79 mm² and 37 mm² regions untouched while six slivers went to
  zero).
- **Corners are preserved.** A plain opening rounds convex corners, and a rounded
  corner makes the scanline emit *extra* stubs — the very artifact being removed
  (+31 on the Voron cube). The surviving core is re-grown by
  `SURFACE_FILL_REGROW_FACTOR (2.0) × radius` and clipped back to the original
  region, restoring the exact shape. That took the cube from +31 stubs to −1.
- Use **`FillRule::NonZero`** for the final clip so CW hole sub-paths stay holes.

This defect is **not** Arachne-specific — both generators produced byte-identical
stub measurements on the caddy hexagon, because it originates in the surface
fill, not the wall generator.

### Redundant gap fill under a solid surface

[`prune_redundant_gap_fill`](surfaces.rs) drops a `GapFill` bead when either a
majority of its vertices lie **inside** `solid_regions`, or it is **sandwiched** —
solid surface on *both* perpendicular sides (`gap_fill_sandwiched_by_surface`).

The sandwich case exists because `blocked_for_surface` unions the gap-fill
footprint *out* of the surface region, carving a bead-wide corridor exactly where
each bead sits. A bead running down the centre of a thin solid strip is therefore
never "inside" the surface, yet the surface's full-width zig-zag still deposits
straight over it. The probe reaches `half-width + 0.5·d` to either side, just past
that corridor: a bead the surface *surrounds* has surface on both probes and is
dropped; a genuine neck that merely *abuts* a surface edge has it on at most one
and is kept.

**The surface must then *cover* the pruned bead's footprint, not carve it out.**
Otherwise the corridor becomes a hole in `solid_regions`, which on a thin roof
splits the top-surface serpentine into two disconnected bands and lets sparse
infill dash across the void — the "two infill surfaces plus tiny blobs of goo"
defect. The corridor was carved from **two** places, so both must stop for a
sandwiched bead: `blocked_for_surface`'s explicit gap-fill term, which uses
`compute_gap_fill_footprint_excluding_sandwiched`; and
`compute_wall_bead_footprint`, which is called with `include_gap_fill = false`
for the surface trim. The sandwich test runs against the layer's **combined**
detected surface, pre-trim, so a centre bead is recognised before the trim would
hole the surface.

> Verify with a true-width **capsule** intersection, not a footprint-erosion
> overlap scan — a thin bead hides from the latter. On the Benchy rear rail that
> hidden double-extrusion was 6 mm²/layer.

### Ironing is a treatment, not material

`add_ironing_for_region` runs inside surface generation, where `top_region` is
still live — and it must touch **no region field**. Ironing is a near-dry
smoothing sweep, not solid material: were its footprint ever folded into
`solid_regions`, `add_infill_to_layers` would subtract it (grown by a full bead)
and punch a hole in the sparse infill underneath.

Two more choices that look like details and are not:

- **It carries its own `ExtrusionRole::Ironing`** rather than reusing
  `TopSurface`. `resolve_width_mm` returns `top_surface_line_width` before it
  ever reads an explicit width, so sharing the role would silently iron at full
  flow on any profile that sets one. A shared role would also merge the two into
  one path-ordering group, letting the TSP interleave ironing with fill that has
  not been printed yet.
- **The flow reduction is folded into the *width*** (`ironing_spacing ×
  ironing_flow`), deliberately keeping it out of `extrusion_for_move`'s
  `flow_ratio` — which reads a non-positive value as `1.0`, so routing a
  "wipe only" setting through it would lay a full-width bead at 0.1 mm pitch.

### Which Clipper2 fill rule, and why

| Operation | Rule | Why |
| --- | --- | --- |
| Surface detection (intersect / difference of layer perimeters) | `EvenOdd` | The mesh slicer does not guarantee consistent winding; EvenOdd is winding-independent |
| Infill interior subtraction (infill area − solid regions) | `Positive` | Input winding is consistent Clipper2 output; `Positive` is more predictable for non-overlapping inputs |
| Wall-footprint subtraction | `NonZero` | The footprint is a frame with CW holes; `Positive` would erase the interior |
| Variable elephant-foot offset cleanup | `Positive` | Discards the reversed folds a variable offset creates in a concavity, while a CW hole still subtracts |

**Do not union Arachne bead paths with `EvenOdd`.** Tightly nested concentric
closed paths under EvenOdd produce alternating in/out bands instead of one solid
region. `NonZero` would work, but only after normalising winding — which is why
that union was removed rather than fixed.

> **Gap-fill length is not bit-reproducible** between runs of the same binary
> (7399.6 vs 7401.8 mm on two Benchy slices), so small gap-fill deltas are noise,
> not evidence. Sparse infill *is* deterministic and can be compared directly.

**So never judge "did my change alter the output?" on a 3DBenchy.** The whole
file moves: three consecutive slices of the *same binary* reported 3924.69,
3924.87 and 3924.86 mm of filament, and `diff` says they differ. A refactor
measured that way looks like a regression when it is noise — and, worse, a real
regression smaller than that spread looks clean.

Use a **deterministic fixture** and compare the G-code byte for byte, skipping
the timestamp header. `Voron_Design_Cube_v7.stl`, `bottom_panel_hinge_x2.stl`
and `Filament_Card_Caddy_25.stl` all reproduce exactly. The quality gate's
tolerances exist to absorb the Benchy's jitter, so **a passing gate is not
evidence that output is unchanged** — only a byte-compare on a deterministic
fixture is.

---

## Spiral (vase) mode

`spiral_vase` prints a single continuous outer wall whose Z ramps over each
layer. It is split across two boundaries so every runtime behaves identically.

**Normalization** ([`SlicingParams::spiral_vase_normalized`](../settings/params.rs))
forces the incompatible settings off — `wall_count = 1`, `infill_density = 0`,
`top_layers = 0`, `retract_mm = 0`, `z_hop_mm = 0`, `ironing_enabled = false` —
while **keeping `bottom_layers`** as the solid base. It is a `Cow` (a no-op
borrow when the flag is off) and **idempotent**, so it is applied at both
boundaries without double effect: the top of `process_mesh` / `process_mesh_debug`,
and the top of `generate_with_stats`.

**The pipeline skips `classify_overhang_perimeters` in spiral mode.** That pass
splits closed wall loops into open arcs, and the spiral emitter needs each
layer's outer wall to stay one closed loop. Nothing else changes — surface
generation still runs for the base, and everything else is a plain single-wall
slice.

**The generator owns the spiralization**; see
[`gcode`](../gcode/README.md). Multi-island layers fall back to a normal flat
print with a single warning — spiral vase is for solid, single-island models.

---

## Critical invariants

These have all been hit as bugs at least once. Read before changing
[`pipeline.rs`](pipeline.rs).

### 1. Snapshot infill regions _before_ wall stripping

Even though the strip is per-island, the snapshot is the safety net that
keeps `calculate_interior_region` honest if the strip ever changes again.
Computing `pre_strip_infill_regions` after the strip would cause sparse
infill to expand into the (now wall-less) zone on stripped islands.

### 2. Surfaces depend on Arachne `OuterWall` paths only

[`surfaces`](surfaces.rs) calls `perimeter_paths_of()`, which intentionally
returns only `OuterWall` paths. Including `InnerWall` beads makes the
EvenOdd fill rule see alternating in/out bands between concentric beads —
phantom "exposed" strips appear and get tagged as top/bottom surfaces.

### 3. `calculate_interior_region` preserves winding

`OuterWall` paths from holes are legitimately CW. Normalising them to CCW
before the inward inflate makes Clipper2 treat hole interiors as solid —
infill is then generated through the void. The `−0.5 × d` correction in
the inflate accounts for the fact that `OuterWall` centerlines are already
inset half a bead width from the model surface.

### 4. Compensation runs on contours, before walls

[`apply_dimensional_compensation`](compensation.rs) is the **first** thing to
touch a layer, and it must stay there. It rewrites a layer's entire path list,
which is only sound while no per-path metadata exists to keep in step with it —
a `debug_assert!` pins that. Run it after wall generation and it would correct
the beads while leaving the interior, and every solid surface with it, sized to
the uncorrected model.

### 5. Elephant foot is medial-limited, never uniform

A uniform inward offset of 0.2 mm deletes every first-layer feature narrower
than 0.4 mm — embossed text, logo strokes, thin ribs — which is exactly the
detail a first layer is judged on. So the shrink is computed per contour vertex
from the largest circle that fits inside the material there, and applied as a
**variable** offset: a feature ends up `max(w_min, w − 2δ)` wide, so nothing thin
is erased.

Three further rules keep it honest, and each exists because the naive version
gets it wrong:

- **Two measurements, not one.** The largest circle that fits inside the
  material *touching* a point collapses toward zero all along a convex corner —
  true, but useless as a limit, because it would leave an uncompensated nub on
  every corner of every model. So the module takes a second reading that counts
  only surfaces which actually **face** the point, as an opposite wall does and
  a corner's adjoining edge does not. The first is restored by a running maximum
  along the contour; the second caps that maximum back down so a thick body's
  radius cannot leak down an attached rib and pinch it off at the root. Their
  failure modes are disjoint, and the smaller of the two is right in both cases.
- **Smoothing may only reduce.** Averaging alone would raise the shrink at a
  thin spot back toward its thicker neighbours, re-eroding the feature the limit
  just protected.
- **Vertices move to the mitre point, not along the normal.** A right-angle
  corner needs `√2 · δ` of travel along its bisector for both of its edges to
  end up `δ` further in; displacing by `δ` would round every corner off.

The **cliff guard** is a separate limit on top: compensation is withheld where
the layer above flares steeply outward past this one, so a narrow pedestal under
a wide body is never undercut. The flare is measured *along the outward normal*,
by `ray_exit_distance`, which walks out of the layer above until its material
ends — the nearest boundary
in any direction answers a different question, and a rim running tangentially
past would hide a deep overhang behind it. It reads the model's own geometry,
which is why the pass walks bottom-up: layer `i` consults layer `i + 1` before
layer `i + 1` is itself rewritten.

See [`../walls/README.md`](../walls/README.md) for the wall-side
implications.

---

## What this module deliberately does _not_ do

- **No mesh placement.** Transforms are baked into the mesh by
  [`scene::transform::apply_transform`](../scene/transform.rs) before
  `process_mesh` runs.
- **No G-code emission.** `Vec<SliceLayer>` goes to [`gcode::`](../gcode/),
  not to disk.
- **No file I/O.** The CLI loads the mesh; the pipeline doesn't know paths
  exist.
- **No profile validation.** `SlicingParams` arrives already validated by
  [`settings::validator`](../settings/validator.rs).
- **No hole-specific dimensional compensation.** Growing holes by their own
  delta needs per-contour hole classification and a setting of its own;
  [`compensation.rs`](compensation.rs) offsets holes only as part of the whole
  cross-section.

---

## See also

- [pipeline.rs](pipeline.rs) — `process_mesh` orchestrator
- [slicer.rs](slicer.rs) — triangle-plane intersection, segment chaining
- [compensation.rs](compensation.rs) — XY size + medial-limited elephant foot
- [walls.rs](walls.rs) — per-island first/top single-wall restriction
- [surfaces.rs](surfaces.rs) — top / bottom solid surface detection and infill
- [infill.rs](infill.rs) — `calculate_interior_region`, sparse infill driver
- [types.rs](types.rs) — `SliceLayer`, `ExtrusionRole`
- [../walls/README.md](../walls/README.md) — wall generation (Arachne + classic)
- [../infill/README.md](../infill/README.md) — sparse infill pattern catalog
- [../SLICING.md](../SLICING.md) — slicing-algorithm walkthrough
- [../../AGENTS.md](../../AGENTS.md) — the repo map
