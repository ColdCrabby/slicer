#!/usr/bin/env python3
"""Render a gcode layer with wall-zone gaps highlighted, for visual diagnosis."""
import sys
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
sys.path.insert(0, __file__.rsplit("/", 1)[0])
from voids import parse_layers, rasterize, fill_holes, dilate, erode, RES, GAP_MAX

TARGET = int(sys.argv[2]) if len(sys.argv) > 2 else 60

COLOR = {
    "Outer wall": ("black", 1.6),
    "Inner wall": ("dimgray", 1.2),
    "Overhang wall": ("purple", 1.2),
    "Gap infill": ("green", 1.0),
    "Sparse infill": ("lightblue", 0.6),
    "Top surface": ("orange", 0.6),
    "Bottom surface": ("gold", 0.6),
    "Bridge": ("cyan", 0.6),
}


def gap_mask(segs):
    xs = [s[0] for s in segs] + [s[2] for s in segs]
    ys = [s[1] for s in segs] + [s[3] for s in segs]
    ox, oy = min(xs) - 2, min(ys) - 2
    W = int((max(xs) + 2 - ox) / RES) + 1
    H = int((max(ys) + 2 - oy) / RES) + 1
    wt = ("Outer wall", "Inner wall", "Overhang wall", "Gap infill")
    wall_cov = rasterize([s for s in segs if s[5] in wt], ox, oy, W, H)
    inf_cov = rasterize([s for s in segs if s[5] not in wt], ox, oy, W, H)
    voids = fill_holes(wall_cov | inf_cov)
    rc = int((GAP_MAX / 2) / RES)
    thin = voids & ~dilate(erode(voids, rc), rc)
    wall_gap = thin & dilate(wall_cov, rc) & ~dilate(inf_cov, rc)
    return wall_gap, ox, oy


def render(ax, path, title):
    segs = parse_layers(path)[TARGET]
    for (x0, y0, x1, y1, w, typ) in segs:
        c, lw = COLOR.get(typ, ("red", 0.5))
        ax.plot([x0, x1], [y0, y1], color=c, lw=lw, solid_capstyle="round")
    wall_gap, ox, oy = gap_mask(segs)
    ys, xs = np.where(wall_gap)
    ax.scatter(ox + (xs + 0.5) * RES, oy + (ys + 0.5) * RES, s=4, c="red", marker="s", zorder=5)
    ax.set_aspect("equal")
    ax.set_title(f"{title}  (red = wall-zone gap)")
    ax.invert_yaxis()


fig, axes = plt.subplots(1, 2, figsize=(22, 12))
render(axes[0], sys.argv[1], "ARACHNE")
render(axes[1], sys.argv[3] if len(sys.argv) > 3 else sys.argv[1], "CLASSIC")
plt.tight_layout()
out = sys.argv[4] if len(sys.argv) > 4 else "/tmp/layer.png"
plt.savefig(out, dpi=130)
print("wrote", out)
