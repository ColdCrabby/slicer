#!/usr/bin/env python3
"""Detect cross-role extrusion overlap (double-extrusion) per gcode layer.

`coincident.py` catches two beads of the *same* wall role running parallel and
too close.  This tool catches the complementary defect the user flagged: a bead
of one role laid on top of a bead of a *different* role — e.g. sparse infill
re-extruding over an existing gap-fill bead — regardless of the angle they cross
at.

Method: rasterise each role's capsule footprint (at its real ``;WIDTH:``) into
its own mask, then intersect masks pairwise.  Adjacent roles legitimately *touch*
along their shared boundary (infill meets the inner wall by design, with
``infill_overlap_percent`` of intended bond), so a raw intersection over-reports.
We therefore also report the **core** overlap: the shared area that survives
eroding each footprint by ~¼ nozzle.  Erosion strips the one-bead-edge boundary
seam and leaves only genuine *body-on-body* overlap — the metric that should be
≈0 for clean toolpaths.

Usage: ``overlap.py <gcode> [layer|all] [--csv]``
"""
import sys

import numpy as np

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from voids import RES, NOZ, parse_layers, rasterize, erode

# Roles that deposit plastic, coarsest first so the printed matrix reads top-down.
ROLES = [
    "Outer wall",
    "Inner wall",
    "Overhang wall",
    "Gap infill",
    "Sparse infill",
    "Top surface",
    "Bottom surface",
    "Bridge",
]

# Erosion radius (cells) that removes the expected shared-boundary seam between
# two adjacent beads: rasterize marks cells within r+½·RES of a centerline, so two
# beads at nominal spacing share ~1 cell; eroding one cell from each strips it.
ERODE_RC = max(1, round((0.25 * NOZ) / RES))
# Report threshold: below this a "body" overlap is rasterisation noise, not a real
# double-extrusion (one cell = RES², a handful of cells is sub-bead jitter).
BODY_EPS = 0.03  # mm^2


def layer_overlaps(segs):
    """Return {(roleA, roleB): (raw_mm2, body_mm2)} for one layer's segments."""
    if not segs:
        return {}
    xs = [s[0] for s in segs] + [s[2] for s in segs]
    ys = [s[1] for s in segs] + [s[3] for s in segs]
    ox, oy = min(xs) - 2, min(ys) - 2
    W = int((max(xs) + 2 - ox) / RES) + 1
    H = int((max(ys) + 2 - oy) / RES) + 1

    cov = {}
    for r in ROLES:
        rs = [s for s in segs if s[5] == r]
        if rs:
            cov[r] = rasterize(rs, ox, oy, W, H)
    core = {r: erode(c, ERODE_RC) for r, c in cov.items()}

    ca = RES * RES
    out = {}
    present = list(cov)
    for i in range(len(present)):
        for j in range(i + 1, len(present)):
            a, b = present[i], present[j]
            raw = float((cov[a] & cov[b]).sum()) * ca
            if raw <= 0.0:
                continue
            body = float((core[a] & core[b]).sum()) * ca
            out[(a, b)] = (raw, body)
    return out


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "arachne.gcode"
    which = sys.argv[2] if len(sys.argv) > 2 and not sys.argv[2].startswith("--") else "all"
    layers = parse_layers(path)
    sel = range(len(layers)) if which == "all" else [int(which)]
    totals = {}  # pair -> [raw, body, layers_with_body]
    for li in sel:
        ov = layer_overlaps(layers[li])
        for k, (raw, body) in ov.items():
            t = totals.setdefault(k, [0.0, 0.0, 0])
            t[0] += raw
            t[1] += body
            if body > BODY_EPS:
                t[2] += 1
                if which != "all":
                    print(f"  L{li}: {k[0]} x {k[1]}: raw {raw:.2f} mm^2, BODY {body:.2f} mm^2")

    n = len(list(sel))
    print(f"\n{path.split('/')[-1]}: cross-role overlap over {n} layer(s)")
    print(f"  (BODY = overlap surviving ¼-nozzle erosion = genuine double-extrusion)\n")
    print(f"{'role pair':38s} {'raw mm^2':>10s} {'BODY mm^2':>10s} {'layers':>7s}")
    print("-" * 68)
    ranked = sorted(totals, key=lambda k: -totals[k][1])
    any_body = False
    for k in ranked:
        raw, body, nl = totals[k]
        if body > BODY_EPS or raw > 1.0:
            flag = "  <-- double-extrusion" if body > BODY_EPS else ""
            print(f"{k[0] + ' x ' + k[1]:38s} {raw:10.1f} {body:10.2f} {nl:7d}{flag}")
            any_body = any_body or body > BODY_EPS
    if not any_body:
        print("(no body-on-body overlap above threshold — only expected boundary touch)")


if __name__ == "__main__":
    main()
