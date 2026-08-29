#!/usr/bin/env python3
"""Before/after bead diff: render one layer from TWO gcode files side by side,
drawing every extrusion as a filled capsule at its ACTUAL ``;WIDTH:``.

This is the "visual double check" for geometry changes. Centerline plots lie:
two beads whose centerlines sit 0.3 mm apart look like tidy separate lines, but
at their real 0.4-0.56 mm widths they are visibly *the same material laid twice*.
Only a true-width capsule render makes double-extrusion, uncovered slivers and
split/fragmented surfaces obvious to a human reviewer.

Short isolated paths (the "tiny extrude / splat" defect class) are highlighted in
magenta and counted in each title, because they are the single most common thing
worth eyeballing after a fill/wall change.

Usage
-----
    beaddiff.py <before.gcode> <after.gcode> [layer=60] [out.png]
                [cx cy half] [--short=0.8] [--titles="Before|After"]

``layer`` is a 1-based dense print-layer index (Z-bucketed, same convention as
the other scripts here). Omit ``cx cy half`` to auto-fit both layers to a shared
window; pass them to zoom on a feature. Both panels always share one scale so
the comparison is honest.

Examples
--------
    # whole layer, auto-fit
    beaddiff.py /tmp/before.gcode /tmp/after.gcode 41 /tmp/diff.png

    # zoom a 3 mm window on the rear rail, flag anything under 1.5 mm
    beaddiff.py /tmp/before.gcode /tmp/after.gcode 201 /tmp/rail.png 0.9 -12 3 --short=1.5
"""
import sys
import math

import matplotlib

matplotlib.use("Agg")
import matplotlib.patches as mpatches
import matplotlib.pyplot as plt
from matplotlib.collections import PatchCollection
from matplotlib.patches import Polygon as MPoly

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from voids import parse_layers  # noqa: E402  (shared Z-bucketed parser)

# Role -> (facecolor, alpha).  Walls are neutral greys so the *fills* — where
# geometry bugs live — carry the colour.
COLOR = {
    "Outer wall": ((0.35, 0.35, 0.35), 0.55),
    "Inner wall": ((0.25, 0.35, 0.85), 0.50),
    "Overhang wall": ((0.00, 0.70, 0.80), 0.55),
    "Gap infill": ((0.00, 0.70, 0.00), 0.60),
    "Sparse infill": ((1.00, 0.60, 0.10), 0.60),
    "Top surface": ((0.90, 0.30, 0.30), 0.45),
    "Bottom surface": ((0.60, 0.20, 0.70), 0.45),
    "Bridge": ((0.10, 0.45, 0.90), 0.55),
}
SHORT_COLOR = "magenta"
DEFAULT_SHORT_MM = 0.8


def capsule(x0, y0, x1, y1, w):
    """Rectangle of width `w` about the segment (rounded caps are visually
    irrelevant at these scales and cost a lot of patches)."""
    dx, dy = x1 - x0, y1 - y0
    L = math.hypot(dx, dy)
    if L < 1e-9:
        return None
    r = w / 2.0
    nx, ny = -dy / L * r, dx / L * r
    return [(x0 + nx, y0 + ny), (x1 + nx, y1 + ny), (x1 - nx, y1 - ny), (x0 - nx, y0 - ny)]


def group_paths(segs):
    """Group a layer's ordered segments into contiguous extrusion *paths*.

    `parse_layers` yields segments only; a path is a maximal run of segments
    that share a role and are head-to-tail connected. Path length — not segment
    length — is what determines whether an extrusion is an isolated "splat"
    (a path is printed in one go; a segment is just one vertex-to-vertex hop).
    """
    paths = []
    cur = None
    for (x0, y0, x1, y1, w, typ) in segs:
        if (
            cur is not None
            and cur["typ"] == typ
            and abs(cur["end"][0] - x0) < 1e-6
            and abs(cur["end"][1] - y0) < 1e-6
        ):
            cur["segs"].append((x0, y0, x1, y1, w))
            cur["len"] += math.hypot(x1 - x0, y1 - y0)
            cur["end"] = (x1, y1)
        else:
            if cur is not None:
                paths.append(cur)
            cur = {
                "typ": typ,
                "segs": [(x0, y0, x1, y1, w)],
                "len": math.hypot(x1 - x0, y1 - y0),
                "end": (x1, y1),
            }
    if cur is not None:
        paths.append(cur)
    return paths


def layer_segs(path, layer_1based):
    layers = parse_layers(path)
    if not layers:
        sys.exit(f"no layers parsed from {path}")
    i = max(0, min(len(layers) - 1, layer_1based - 1))
    if i != layer_1based - 1:
        print(f"warning: {path} has {len(layers)} layers; clamped to {i + 1}")
    return layers[i]


def bounds(*segs_lists):
    xs, ys = [], []
    for segs in segs_lists:
        for (x0, y0, x1, y1, _w, _t) in segs:
            xs += [x0, x1]
            ys += [y0, y1]
    if not xs:
        return (-10, 10, -10, 10)
    return min(xs), max(xs), min(ys), max(ys)


def draw(ax, segs, title, short_mm):
    n_short = 0
    patches, colors, alphas = [], [], []
    for p in group_paths(segs):
        is_short = p["len"] < short_mm
        if is_short:
            n_short += 1
        face, alpha = COLOR.get(p["typ"], ((0.5, 0.5, 0.5), 0.5))
        if is_short:
            face, alpha = SHORT_COLOR, 0.95
        for (x0, y0, x1, y1, w) in p["segs"]:
            poly = capsule(x0, y0, x1, y1, w)
            if poly:
                patches.append(MPoly(poly))
                colors.append(face)
                alphas.append(alpha)
        if is_short:
            ax.plot(
                p["segs"][0][0], p["segs"][0][1],
                "o", mfc="none", mec=SHORT_COLOR, ms=13, mew=1.6, zorder=5,
            )
    # One PatchCollection per alpha bucket keeps rendering fast while still
    # letting fills sit under the wall greys at their own transparency.
    for a in sorted(set(alphas)):
        sel = [p for p, al in zip(patches, alphas) if al == a]
        col = [c for c, al in zip(colors, alphas) if al == a]
        ax.add_collection(PatchCollection(sel, facecolor=col, edgecolor="none", alpha=a))
    ax.set_title(f"{title}\n({n_short} isolated paths < {short_mm:g} mm)", fontsize=11)
    ax.set_aspect("equal")
    ax.grid(True, alpha=0.25)
    return n_short


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    opts = {a.split("=", 1)[0]: a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--") and "=" in a}
    if len(args) < 2:
        sys.exit(__doc__)

    before, after = args[0], args[1]
    layer = int(args[2]) if len(args) > 2 else 60
    out = args[3] if len(args) > 3 else "/tmp/beaddiff.png"
    cx = float(args[4]) if len(args) > 4 else None
    cy = float(args[5]) if len(args) > 5 else None
    half = float(args[6]) if len(args) > 6 else None
    short_mm = float(opts.get("--short", DEFAULT_SHORT_MM))
    titles = opts.get("--titles", "BEFORE|AFTER").split("|")
    t_before = titles[0]
    t_after = titles[1] if len(titles) > 1 else "AFTER"

    sa = layer_segs(before, layer)
    sb = layer_segs(after, layer)

    if cx is not None and cy is not None and half is not None:
        # Explicit zoom: a square window, so the two panels read best side by side.
        xlim = (cx - half, cx + half)
        ylim = (cy - half, cy + half)
    else:
        # Auto-fit to the data with a small margin.  Deliberately *not* squared:
        # a long flat feature (a rail roof, a deck edge) must render wide, or the
        # detail the reviewer is meant to check collapses to a few pixels.
        minx, maxx, miny, maxy = bounds(sa, sb)
        mx = max(0.5, (maxx - minx) * 0.03)
        my = max(0.5, (maxy - miny) * 0.03)
        xlim = (minx - mx, maxx + mx)
        ylim = (miny - my, maxy + my)

    # Stack vertically for wide/flat windows, side by side otherwise — a wide
    # part squeezed into a narrow column is unreadable in a PR.
    wide = (xlim[1] - xlim[0]) > 2.0 * (ylim[1] - ylim[0])
    if wide:
        fig, axes = plt.subplots(2, 1, figsize=(17, 8))
    else:
        fig, axes = plt.subplots(1, 2, figsize=(19, 10))

    n_b = draw(axes[0], sa, t_before, short_mm)
    n_a = draw(axes[1], sb, t_after, short_mm)
    for ax in axes:
        ax.set_xlim(*xlim)
        ax.set_ylim(*ylim)

    roles = sorted({p["typ"] for p in group_paths(sa)} | {p["typ"] for p in group_paths(sb)})
    handles = [
        mpatches.Patch(color=COLOR[r][0], label=r) for r in roles if r in COLOR
    ] + [mpatches.Patch(color=SHORT_COLOR, label=f"isolated path < {short_mm:g} mm")]
    fig.legend(handles=handles, loc="lower center", ncol=min(6, len(handles)), frameon=False)
    fig.suptitle(f"layer {layer} — beads drawn at their true ;WIDTH:", fontsize=12)
    plt.tight_layout(rect=[0, 0.05, 1, 0.97])
    plt.savefig(out, dpi=95, bbox_inches="tight")
    print(f"layer {layer}: isolated paths < {short_mm:g} mm  {n_b} -> {n_a}")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
