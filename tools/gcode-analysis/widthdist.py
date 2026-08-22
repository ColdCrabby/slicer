#!/usr/bin/env python3
"""Length-weighted extrusion-width distribution per role, for one gcode file.

Marker-count stats over-represent short shed corners; weighting each width by the
mm of travel it actually applies to shows whether walls are mostly full-width.
"""
import sys

PATH = sys.argv[1] if len(sys.argv) > 1 else "arachne.gcode"
ROLE = sys.argv[2] if len(sys.argv) > 2 else "wall"  # wall|gap|all
import math

x = y = 0.0
typ = "?"
width = None
# width -> total length
buckets = {}


def want(t):
    if ROLE == "wall":
        return t in ("Outer wall", "Inner wall", "Overhang wall")
    if ROLE == "gap":
        return t == "Gap infill"
    return True


with open(PATH) as f:
    for line in f:
        line = line.strip()
        if line.startswith(";TYPE:"):
            typ = line[6:].strip()
            continue
        if line.startswith(";WIDTH:"):
            try:
                width = float(line[7:].replace("mm", "").strip())
            except ValueError:
                width = None
            continue
        if not line or line[0] == ";":
            continue
        if line.startswith("G1") or line.startswith("G0"):
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
                elif c == "E":
                    e = v
            if e is not None and e > 0 and width is not None and want(typ):
                ln = math.hypot(x - px, y - py)
                key = round(width, 2)
                buckets[key] = buckets.get(key, 0.0) + ln

total = sum(buckets.values())
if total == 0:
    print("no extrusion for role", ROLE)
    sys.exit()
print(f"role={ROLE} total_len={total:.1f}mm across {len(buckets)} width bins")
cum = 0.0
for w in sorted(buckets):
    ln = buckets[w]
    cum += ln
    bar = "#" * int(50 * ln / max(buckets.values()))
    print(f"  {w:.2f}mm : {ln:8.1f}mm ({100*ln/total:5.1f}%) cum {100*cum/total:5.1f}%  {bar}")
