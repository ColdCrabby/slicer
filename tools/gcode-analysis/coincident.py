#!/usr/bin/env python3
"""Measure coincident (overlapping) wall beads on a given layer of a gcode file.

A pair of extrusion segments is "coincident" when they are near-parallel and the
perpendicular distance between them is below a gap threshold while their
projections overlap.  Two beads that close together over-extrude no matter how
thin they are, so this is the metric that must go to zero for clean walls.
"""
import sys
import math

PATH = sys.argv[1] if len(sys.argv) > 1 else "arachne.gcode"
TARGET_LAYER = int(sys.argv[2]) if len(sys.argv) > 2 else 60
GAP = float(sys.argv[3]) if len(sys.argv) > 3 else 0.10  # mm


def parse():
    """Bucket extrusion segments by the Z at which they are laid down, so Z-lift
    travel moves don't shatter the model into thousands of pseudo-layers."""
    buckets = {}  # rounded-z -> list of (x0,y0,x1,y1,type)
    x = y = z = 0.0
    typ = "?"
    with open(PATH) as f:
        for line in f:
            line = line.strip()
            if line.startswith(";TYPE:"):
                typ = line[6:].strip()
                continue
            if not line or line[0] == ";":
                continue
            if line[0] == "G" and (line.startswith("G1") or line.startswith("G0")):
                px, py = x, y
                e = None
                for tok in line.split()[1:]:
                    c = tok[0]
                    try:
                        v = float(tok[1:])
                    except ValueError:
                        continue
                    if c == "X":
                        x = v
                    elif c == "Y":
                        y = v
                    elif c == "Z":
                        z = v
                    elif c == "E":
                        e = v
                if e is not None and e > 0 and (x != px or y != py):
                    key = round(z, 2)
                    buckets.setdefault(key, []).append((px, py, x, y, typ))
    return [buckets[k] for k in sorted(buckets)]


def seg_dist_parallel(a, b):
    """Return (perp_dist, overlap) if a,b are near-parallel & projections overlap,
    else None."""
    ax0, ay0, ax1, ay1, _ = a
    bx0, by0, bx1, by1, _ = b
    dax, day = ax1 - ax0, ay1 - ay0
    dbx, dby = bx1 - bx0, by1 - by0
    la = math.hypot(dax, day)
    lb = math.hypot(dbx, dby)
    if la < 1e-6 or lb < 1e-6:
        return None
    # unit dir of a
    ux, uy = dax / la, day / la
    # angle between (allow anti-parallel)
    cross = abs((dax * dby - day * dbx) / (la * lb))
    if cross > 0.20:  # ~11.5 deg
        return None
    # project b endpoints onto a's line; perpendicular distances
    def perp(px, py):
        # vector from a0 to p, cross with unit dir = signed perp distance
        return abs((px - ax0) * uy - (py - ay0) * ux)

    def proj(px, py):
        return (px - ax0) * ux + (py - ay0) * uy

    d0 = perp(bx0, by0)
    d1 = perp(bx1, by1)
    pd = 0.5 * (d0 + d1)
    # overlap in projection
    pb0, pb1 = proj(bx0, by0), proj(bx1, by1)
    lo = max(0.0, min(la, max(pb0, pb1)) - max(0.0, min(pb0, pb1)))
    if lo < 0.2:  # need meaningful overlap length (mm)
        return None
    return pd, lo


def analyze(layer, role_filter):
    segs = [s for s in layer if role_filter(s[4])]
    n = len(segs)
    coincident = 0
    total_len = 0.0
    seen = set()
    for i in range(n):
        for j in range(i + 2, n):  # skip adjacent
            r = seg_dist_parallel(segs[i], segs[j])
            if r is None:
                continue
            pd, lo = r
            if pd < GAP:
                key = (i, j)
                if key not in seen:
                    seen.add(key)
                    coincident += 1
                    total_len += lo
    return n, coincident, total_len


def main():
    layers = parse()
    print(f"parsed {len(layers)} layers from {PATH}")
    if TARGET_LAYER >= len(layers):
        print(f"layer {TARGET_LAYER} out of range")
        return
    layer = layers[TARGET_LAYER]
    for name, filt in [
        ("inner-vs-inner", lambda t: t == "Inner wall"),
        ("outer-vs-outer", lambda t: t == "Outer wall"),
        ("any-wall", lambda t: t in ("Inner wall", "Outer wall", "Overhang wall")),
    ]:
        n, c, tl = analyze(layer, filt)
        print(f"  layer {TARGET_LAYER} {name:16s}: {n:4d} segs, {c:4d} coincident pairs, {tl:7.2f}mm overlap")


if __name__ == "__main__":
    main()
