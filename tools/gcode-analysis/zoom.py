#!/usr/bin/env python3
"""Render a zoomed gcode region drawing every bead at its ACTUAL ;WIDTH: as a
filled capsule, so we can see whether gap-fill beads truly span their gaps."""
import sys
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Polygon as MPoly
sys.path.insert(0, __file__.rsplit("/", 1)[0])
from voids import parse_layers

PATH = sys.argv[1]
TARGET = int(sys.argv[2]) if len(sys.argv) > 2 else 60
# zoom window cx,cy,half
CX = float(sys.argv[3]) if len(sys.argv) > 3 else 0.0
CY = float(sys.argv[4]) if len(sys.argv) > 4 else 0.0
HALF = float(sys.argv[5]) if len(sys.argv) > 5 else 8.0
OUT = sys.argv[6] if len(sys.argv) > 6 else "/tmp/zoom.png"

COLOR = {
    "Outer wall": ("0.2", 0.7),
    "Inner wall": ("0.45", 0.7),
    "Overhang wall": ("purple", 0.7),
    "Gap infill": ("green", 0.55),
    "Sparse infill": ("skyblue", 0.4),
    "Top surface": ("orange", 0.4),
    "Bottom surface": ("gold", 0.4),
}


def capsule(x0, y0, x1, y1, w):
    dx, dy = x1 - x0, y1 - y0
    L = np.hypot(dx, dy)
    if L < 1e-9:
        return None
    ux, uy = dx / L, dy / L
    px, py = -uy, ux
    r = w / 2
    return [
        (x0 + px * r, y0 + py * r),
        (x1 + px * r, y1 + py * r),
        (x1 - px * r, y1 - py * r),
        (x0 - px * r, y0 - py * r),
    ]


segs = parse_layers(PATH)[TARGET]
fig, ax = plt.subplots(figsize=(14, 14))
for (x0, y0, x1, y1, w, typ) in segs:
    c, a = COLOR.get(typ, ("red", 0.4))
    poly = capsule(x0, y0, x1, y1, w)
    if poly:
        ax.add_patch(MPoly(poly, closed=True, facecolor=c, edgecolor="none", alpha=a))
# centerlines of gap fill on top (dashed) to see path vs footprint
for (x0, y0, x1, y1, w, typ) in segs:
    if typ == "Gap infill":
        ax.plot([x0, x1], [y0, y1], color="darkgreen", lw=0.6, zorder=6)
ax.set_xlim(CX - HALF, CX + HALF)
ax.set_ylim(CY - HALF, CY + HALF)
ax.set_aspect("equal")
ax.invert_yaxis()
ax.set_title(f"{PATH.split('/')[-1]} L{TARGET} @({CX},{CY})±{HALF}  gap=green footprint at real WIDTH")
plt.tight_layout()
plt.savefig(OUT, dpi=150)
print("wrote", OUT)
