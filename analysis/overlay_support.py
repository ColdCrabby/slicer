#!/usr/bin/env python3
"""Overlay layer L's Bridge/target-role beads on layer L-1's full footprint.

Answers "is this extrusion laid over thin air?": grey = everything printed on the
layer below (the support), colored = the role of interest on layer L. Any colored
bead not sitting on grey is extruding into space.

Usage: overlay_support.py <gcode> <layer> [role=Bridge] [out.png]
"""
import sys
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

sys.path.insert(0, __file__.rsplit("/", 1)[0] + "/../tools/gcode-analysis")
from voids import parse_layers

path = sys.argv[1]
L = int(sys.argv[2])
role = sys.argv[3] if len(sys.argv) > 3 else "Bridge"
out = sys.argv[4] if len(sys.argv) > 4 else "/tmp/overlay.png"

layers = parse_layers(path)
below = layers[L - 1] if L > 0 else []
cur = layers[L]

fig, ax = plt.subplots(figsize=(12, 12))
# Support from the layer below: every deposited bead, thick pale grey.
for (x0, y0, x1, y1, w, typ) in below:
    ax.plot([x0, x1], [y0, y1], color="0.80", lw=4, solid_capstyle="round", zorder=1)
# Current layer's walls for reference (thin dark outline).
for (x0, y0, x1, y1, w, typ) in cur:
    if typ in ("Outer wall", "Inner wall", "Overhang wall"):
        ax.plot([x0, x1], [y0, y1], color="navy", lw=0.6, zorder=2)
# The role of interest on the current layer, bright red.
n = 0
for (x0, y0, x1, y1, w, typ) in cur:
    if typ == role:
        ax.plot([x0, x1], [y0, y1], color="red", lw=1.4, solid_capstyle="round", zorder=3)
        n += 1
ax.set_aspect("equal")
ax.invert_yaxis()
ax.set_title(f"{path.split('/')[-1]} L{L}: {role} (red, {n} segs) over L{L-1} footprint (grey)")
plt.tight_layout()
plt.savefig(out, dpi=130)
print(f"wrote {out}  ({role} segs on L{L}: {n})")
