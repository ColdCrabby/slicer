#!/usr/bin/env python3
"""Annotate one layer's islands and the wall band of a chosen island.

Built to pin down "a wrong zig-zag between the two inner walls" reports: it
labels every island, then zooms one of them and draws the wall loops in
**print order** (outer, inner-1, inner-2, ...) as separate colours so "between
the two inner walls" is unambiguous, with every bead at its true ``;WIDTH:``.

Usage
-----
    wallbands.py <gcode> <layer> [island_index] [out.png]

`island_index` selects which outer-wall loop to zoom (see the overview panel's
labels); omit it to zoom the smallest island.
"""
import math
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.patches as mpatches
import matplotlib.pyplot as plt
from matplotlib.collections import PatchCollection
from matplotlib.patches import Polygon as MPoly

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from voids import parse_layers  # noqa: E402

FILL_ROLES = ("Top surface", "Bottom surface", "Sparse infill", "Bridge")
# Wall loops are coloured by their depth in the shell, not by role, so the
# "first inner" and "second inner" loop are visually distinct.
DEPTH_COLOR = [(0.20, 0.20, 0.20), (0.20, 0.45, 0.90), (0.60, 0.25, 0.85), (0.0, 0.6, 0.6)]
FILL_COLOR = {
    "Top surface": (0.90, 0.30, 0.30),
    "Bottom surface": (0.60, 0.20, 0.70),
    "Sparse infill": (1.00, 0.60, 0.10),
    "Gap infill": (0.00, 0.75, 0.00),
    "Bridge": (0.10, 0.45, 0.90),
}


def paths_of(segs):
    out, cur = [], None
    for (x0, y0, x1, y1, w, t) in segs:
        if (
            cur
            and cur["t"] == t
            and abs(cur["e"][0] - x0) < 1e-6
            and abs(cur["e"][1] - y0) < 1e-6
        ):
            cur["l"] += math.hypot(x1 - x0, y1 - y0)
            cur["e"] = (x1, y1)
            cur["pts"].append((x1, y1))
        else:
            if cur:
                out.append(cur)
            cur = {
                "t": t,
                "l": math.hypot(x1 - x0, y1 - y0),
                "e": (x1, y1),
                "w": w,
                "pts": [(x0, y0), (x1, y1)],
            }
    if cur:
        out.append(cur)
    return out


def bbox(pts):
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    return min(xs), max(xs), min(ys), max(ys)


def contains(outer, inner, pad=0.0):
    ox0, ox1, oy0, oy1 = bbox(outer)
    ix0, ix1, iy0, iy1 = bbox(inner)
    return ox0 - pad <= ix0 and ix1 <= ox1 + pad and oy0 - pad <= iy0 and iy1 <= oy1 + pad


def capsule(x0, y0, x1, y1, w):
    dx, dy = x1 - x0, y1 - y0
    L = math.hypot(dx, dy)
    if L < 1e-9:
        return None
    r = w / 2
    nx, ny = -dy / L * r, dx / L * r
    return [(x0 + nx, y0 + ny), (x1 + nx, y1 + ny), (x1 - nx, y1 - ny), (x0 - nx, y0 - ny)]


def draw_paths(ax, paths, color_of, alpha=0.85):
    pats, cols = [], []
    for p in paths:
        c = color_of(p)
        if c is None:
            continue
        for i in range(len(p["pts"]) - 1):
            poly = capsule(*p["pts"][i], *p["pts"][i + 1], p["w"])
            if poly:
                pats.append(MPoly(poly))
                cols.append(c)
    if pats:
        ax.add_collection(PatchCollection(pats, facecolor=cols, edgecolor="none", alpha=alpha))


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    path, layer = sys.argv[1], int(sys.argv[2])
    island_idx = int(sys.argv[3]) if len(sys.argv) > 3 else None
    out = sys.argv[4] if len(sys.argv) > 4 else "/tmp/wallbands.png"

    layers = parse_layers(path)
    ps = paths_of(layers[layer - 1])
    z = None
    outers = [p for p in ps if p["t"] == "Outer wall"]
    inners = [p for p in ps if p["t"] == "Inner wall"]

    # Group inner loops under the smallest outer loop that contains them, so a
    # nested island doesn't steal the enclosing frame's walls.
    groups = []
    for oi, o in enumerate(outers):
        kids = [
            q
            for q in inners
            if contains(o["pts"], q["pts"], 0.05)
            and not any(
                contains(o2["pts"], q["pts"], 0.05)
                and (bbox(o2["pts"])[1] - bbox(o2["pts"])[0])
                < (bbox(o["pts"])[1] - bbox(o["pts"])[0])
                for o2 in outers
                if o2 is not o
            )
        ]
        # Sort inner loops outward-in by bbox width: inner-1 then inner-2.
        kids.sort(key=lambda q: -(bbox(q["pts"])[1] - bbox(q["pts"])[0]))
        groups.append({"outer": o, "inners": kids, "idx": oi})

    if island_idx is None:
        island_idx = min(
            range(len(groups)),
            key=lambda i: bbox(groups[i]["outer"]["pts"])[1] - bbox(groups[i]["outer"]["pts"])[0],
        )
    g = groups[island_idx]

    fig, axes = plt.subplots(1, 2, figsize=(19, 9.5))

    # ── Panel A: whole layer, islands labelled ───────────────────────────────
    ax = axes[0]
    draw_paths(ax, [p for p in ps if p["t"] in FILL_COLOR and p["t"] != "Gap infill"],
               lambda p: FILL_COLOR.get(p["t"]), alpha=0.35)
    draw_paths(ax, [p for p in ps if p["t"] == "Gap infill"], lambda p: FILL_COLOR["Gap infill"], 0.6)
    draw_paths(ax, outers, lambda p: DEPTH_COLOR[0], 0.9)
    draw_paths(ax, inners, lambda p: DEPTH_COLOR[1], 0.7)
    for gg in groups:
        x0, x1, y0, y1 = bbox(gg["outer"]["pts"])
        sel = gg["idx"] == island_idx
        ax.add_patch(
            mpatches.Rectangle(
                (x0 - 0.6, y0 - 0.6), x1 - x0 + 1.2, y1 - y0 + 1.2,
                fill=False, ec="red" if sel else "0.5",
                lw=2.2 if sel else 1.0, ls="-" if sel else "--", zorder=6,
            )
        )
        ax.text(x0, y1 + 1.2, f"island {gg['idx']} ({len(gg['inners'])} inner)",
                color="red" if sel else "0.35", fontsize=9, fontweight="bold" if sel else "normal")
    ax.set_title(f"A — layer {layer}: all islands (red = zoomed below)", fontsize=11)
    ax.set_aspect("equal")
    ax.grid(True, alpha=0.25)

    # ── Panel B: the chosen island, wall loops by depth ──────────────────────
    ax = axes[1]
    x0, x1, y0, y1 = bbox(g["outer"]["pts"])
    pad = max(1.5, 0.08 * max(x1 - x0, y1 - y0))
    loops = [g["outer"]] + g["inners"]
    inzoom = lambda p: any(x0 - pad <= a[0] <= x1 + pad and y0 - pad <= a[1] <= y1 + pad
                           for a in p["pts"])
    fills = [p for p in ps if p["t"] in FILL_COLOR and inzoom(p)]
    draw_paths(ax, fills, lambda p: FILL_COLOR.get(p["t"]), 0.9)
    for d, loop in enumerate(loops):
        draw_paths(ax, [loop], lambda p, d=d: DEPTH_COLOR[min(d, len(DEPTH_COLOR) - 1)], 0.85)
    short = [p for p in fills if p["l"] < 4.0]
    for p in short:
        ax.plot(p["pts"][0][0], p["pts"][0][1], "o", mfc="none", mec="magenta", ms=13, mew=1.8,
                zorder=8)
    ax.set_xlim(x0 - pad, x1 + pad)
    ax.set_ylim(y0 - pad, y1 + pad)
    ax.set_title(
        f"B — island {island_idx}: 1 outer + {len(g['inners'])} inner wall(s)\n"
        f"{len(fills)} fill paths inside, {len(short)} shorter than 4 mm (magenta rings)",
        fontsize=11,
    )
    ax.set_aspect("equal")
    ax.grid(True, alpha=0.25)

    handles = [mpatches.Patch(color=DEPTH_COLOR[0], label="outer wall")]
    for d in range(1, len(loops)):
        handles.append(
            mpatches.Patch(color=DEPTH_COLOR[min(d, len(DEPTH_COLOR) - 1)], label=f"inner wall {d}")
        )
    for r in sorted({p["t"] for p in fills}):
        handles.append(mpatches.Patch(color=FILL_COLOR[r], label=r))
    handles.append(mpatches.Patch(color="magenta", label="fill path < 4 mm"))
    fig.legend(handles=handles, loc="lower center", ncol=min(7, len(handles)), frameon=False)
    fig.suptitle(f"{path}  —  layer {layer}, beads at true ;WIDTH:", fontsize=12)
    plt.tight_layout(rect=[0, 0.06, 1, 0.95])
    plt.savefig(out, dpi=100, bbox_inches="tight")

    print(f"layer {layer}: {len(groups)} islands")
    for gg in groups:
        bx = bbox(gg["outer"]["pts"])
        print(f"  island {gg['idx']}: {len(gg['inners'])} inner wall(s)  "
              f"x[{bx[0]:.1f},{bx[1]:.1f}] y[{bx[2]:.1f},{bx[3]:.1f}]")
    print(f"zoomed island {island_idx}; wrote {out}")


if __name__ == "__main__":
    main()
