#!/usr/bin/env python3
"""Per-layer probe: bridge presence, and whether a point is covered by material.

Usage: probe_layers.py <gcode> <lo> <hi> [cx cy]
Prints, per layer: Z, #bridge segs, bridge length, and whether (cx,cy) is inside
any extrusion footprint (material) on that layer and the one below.
"""
import sys
import math

sys.path.insert(0, __file__.rsplit("/", 1)[0] + "/../tools/gcode-analysis")
from voids import parse_layers

path = sys.argv[1]
lo = int(sys.argv[2])
hi = int(sys.argv[3])
cx = float(sys.argv[4]) if len(sys.argv) > 4 else None
cy = float(sys.argv[5]) if len(sys.argv) > 5 else None

layers = parse_layers(path)


def near_point(segs, px, py, tol=0.6):
    """True if any bead centerline passes within tol mm of (px,py)."""
    for (x0, y0, x1, y1, w, typ) in segs:
        dx, dy = x1 - x0, y1 - y0
        L2 = dx * dx + dy * dy
        if L2 < 1e-9:
            d = math.hypot(px - x0, py - y0)
        else:
            t = max(0.0, min(1.0, ((px - x0) * dx + (py - y0) * dy) / L2))
            d = math.hypot(px - (x0 + t * dx), py - (y0 + t * dy))
        if d <= tol:
            return typ
    return None


print(f"{path.split('/')[-1]}  layers {lo}..{hi}")
hdr = "  L    Z     #brg  brgLen"
if cx is not None:
    hdr += f"   mat@({cx:.0f},{cy:.0f})  matBelow"
print(hdr)
for i in range(lo, min(hi + 1, len(layers))):
    segs = layers[i]
    brg = [s for s in segs if s[5] == "Bridge"]
    blen = sum(math.hypot(s[2] - s[0], s[3] - s[1]) for s in brg)
    row = f"  {i:<4} ---   {len(brg):<4} {blen:6.1f}"
    if cx is not None:
        here = near_point(segs, cx, cy)
        below = near_point(layers[i - 1], cx, cy) if i > 0 else None
        row += f"   {str(here or '-'):>10}  {str(below or '-'):>10}"
    print(row)
