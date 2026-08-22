#!/usr/bin/env python3
"""Find thin enclosed voids (unfilled wall-zone gaps) per gcode layer.

Rasterises the capsule footprint of every extrusion, finds cells that are
enclosed by material yet uncovered, and keeps only the ones thinner than
`gap_max` — i.e. the leftover gaps a wall/gap-fill bead should have closed
(sparse-infill designed gaps are wider and are filtered out).
"""
import sys
import math
import numpy as np
from collections import deque

RES = 0.08  # mm/cell
NOZ = 0.40
GAP_MAX = 2.5 * NOZ  # 1.0 mm: voids thinner than this are wall-zone gaps


def parse_layers(path):
    buckets = {}
    x = y = z = 0.0
    typ = "?"
    w = NOZ
    for line in open(path):
        line = line.strip()
        if line.startswith(";TYPE:"):
            typ = line[6:].strip()
            continue
        if line.startswith(";WIDTH:"):
            try:
                w = float(line[7:].replace("mm", "").strip())
            except ValueError:
                w = NOZ
            continue
        if not line or line[0] == ";":
            continue
        if line[0] == "G" and (line[:2] in ("G1", "G0")):
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
                buckets.setdefault(round(z, 2), []).append((px, py, x, y, w, typ))
    return [buckets[k] for k in sorted(buckets)]


def rasterize(segs, ox, oy, W, H):
    cov = np.zeros((H, W), dtype=bool)
    for (x0, y0, x1, y1, w, typ) in segs:
        r = w / 2.0
        minx = min(x0, x1) - r
        maxx = max(x0, x1) + r
        miny = min(y0, y1) - r
        maxy = max(y0, y1) + r
        ci0 = max(0, int((minx - ox) / RES))
        ci1 = min(W, int((maxx - ox) / RES) + 1)
        ri0 = max(0, int((miny - oy) / RES))
        ri1 = min(H, int((maxy - oy) / RES) + 1)
        if ci1 <= ci0 or ri1 <= ri0:
            continue
        cx = ox + (np.arange(ci0, ci1) + 0.5) * RES
        cy = oy + (np.arange(ri0, ri1) + 0.5) * RES
        gx, gy = np.meshgrid(cx, cy)
        dx, dy = x1 - x0, y1 - y0
        ll = dx * dx + dy * dy
        if ll < 1e-9:
            dist = np.hypot(gx - x0, gy - y0)
        else:
            t = np.clip(((gx - x0) * dx + (gy - y0) * dy) / ll, 0, 1)
            dist = np.hypot(gx - (x0 + t * dx), gy - (y0 + t * dy))
        cov[ri0:ri1, ci0:ci1] |= dist <= (r + 0.5 * RES)
    return cov


def disk_offsets(rc):
    offs = []
    for dr in range(-rc, rc + 1):
        for dc in range(-rc, rc + 1):
            if dr * dr + dc * dc <= rc * rc:
                offs.append((dr, dc))
    return offs


def dilate(mask, rc):
    out = mask.copy()
    for dr, dc in disk_offsets(rc):
        out |= np.roll(np.roll(mask, dr, 0), dc, 1)
    return out


def fill_holes(cov):
    """Cells NOT reachable from the border through free space = enclosed."""
    H, W = cov.shape
    free = ~cov
    ext = np.zeros_like(cov)
    dq = deque()
    for i in range(H):
        for j in (0, W - 1):
            if free[i, j] and not ext[i, j]:
                ext[i, j] = True
                dq.append((i, j))
    for j in range(W):
        for i in (0, H - 1):
            if free[i, j] and not ext[i, j]:
                ext[i, j] = True
                dq.append((i, j))
    while dq:
        i, j = dq.popleft()
        for di, dj in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            ni, nj = i + di, j + dj
            if 0 <= ni < H and 0 <= nj < W and free[ni, nj] and not ext[ni, nj]:
                ext[ni, nj] = True
                dq.append((ni, nj))
    return free & ~ext  # enclosed free cells


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "arachne.gcode"
    target = int(sys.argv[2]) if len(sys.argv) > 2 else 60
    layers = parse_layers(path)
    if target >= len(layers):
        print("layer out of range", len(layers))
        return
    segs = layers[target]
    xs = [s[0] for s in segs] + [s[2] for s in segs]
    ys = [s[1] for s in segs] + [s[3] for s in segs]
    ox, oy = min(xs) - 2, min(ys) - 2
    W = int((max(xs) + 2 - ox) / RES) + 1
    H = int((max(ys) + 2 - oy) / RES) + 1

    wall_types = ("Outer wall", "Inner wall", "Overhang wall", "Gap infill")
    wg = [s for s in segs if s[5] in wall_types]
    inf = [s for s in segs if s[5] not in wall_types]
    wall_cov = rasterize(wg, ox, oy, W, H)
    inf_cov = rasterize(inf, ox, oy, W, H)
    cov = wall_cov | inf_cov
    voids = fill_holes(cov)  # all enclosed uncovered cells

    rc = int((GAP_MAX / 2) / RES)
    thick = dilate(erode(voids, rc), rc)  # opening: wide cavities survive
    thin_voids = voids & ~thick

    # wall-zone gap = thin void hugging wall/gap coverage but not infill
    near_wall = dilate(wall_cov, rc)
    near_inf = dilate(inf_cov, rc)
    wall_gap = thin_voids & near_wall & ~near_inf

    ca = RES * RES
    print(f"layer {target}: thin void {thin_voids.sum()*ca:6.2f}mm^2 | "
          f"WALL-ZONE gap {wall_gap.sum()*ca:6.2f}mm^2")

    # connected components of wall_gap: are these few long gaps or many short?
    H2, W2 = wall_gap.shape
    seen = np.zeros_like(wall_gap)
    comps = []
    for i in range(H2):
        for j in range(W2):
            if wall_gap[i, j] and not seen[i, j]:
                q = deque([(i, j)])
                seen[i, j] = True
                cells = []
                while q:
                    a, b = q.popleft()
                    cells.append((a, b))
                    for da, db in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)):
                        na, nb = a + da, b + db
                        if 0 <= na < H2 and 0 <= nb < W2 and wall_gap[na, nb] and not seen[na, nb]:
                            seen[na, nb] = True
                            q.append((na, nb))
                rr = [c[0] for c in cells]
                cc = [c[1] for c in cells]
                area = len(cells) * ca
                span = max((max(rr) - min(rr)), (max(cc) - min(cc))) * RES
                comps.append((area, span))
    comps.sort(reverse=True)
    big = [c for c in comps if c[0] > 0.3]
    print(f"   {len(comps)} gap components (> {0.3}mm^2: {len(big)}); "
          f"top: " + ", ".join(f"{a:.1f}mm^2/span{s:.1f}mm" for a, s in comps[:5]))


def erode(mask, rc):
    return ~dilate(~mask, rc)


if __name__ == "__main__":
    main()
