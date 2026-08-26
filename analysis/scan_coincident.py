#!/usr/bin/env python3
"""All-layer wall-coincidence scan reusing coincident.py's geometry.

Usage: scan_coincident.py <gcode> [gap_mm=0.10]
Prints per-layer non-zero coincidence and a whole-model total.
"""
import sys
import importlib.util

HERE = __file__.rsplit("/", 1)[0]
TOOLS = HERE + "/../tools/gcode-analysis"
path = sys.argv[1]
gap = float(sys.argv[2]) if len(sys.argv) > 2 else 0.10

# Load coincident.py with argv primed so its module-level globals resolve.
sys.argv = [TOOLS + "/coincident.py", path, "0", str(gap)]
spec = importlib.util.spec_from_file_location("coinc", TOOLS + "/coincident.py")
coinc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coinc)

layers = coinc.parse()
anywall = lambda t: t in ("Inner wall", "Outer wall", "Overhang wall")

tot_pairs = 0
tot_len = 0.0
worst = (0, 0, 0.0)
bad_layers = 0
for i, layer in enumerate(layers):
    n, c, tl = coinc.analyze(layer, anywall)
    if c > 0:
        bad_layers += 1
        tot_pairs += c
        tot_len += tl
        if tl > worst[2]:
            worst = (i, c, tl)
        print(f"  L{i:>3}: {c:>3} pairs, {tl:7.2f}mm overlap")

name = path.split("/")[-1]
print(f"\n{name}: {len(layers)} layers  |  {bad_layers} layers with coincidence")
print(f"  total: {tot_pairs} pairs, {tot_len:.2f}mm wall-on-wall overlap")
if worst[1]:
    print(f"  worst: L{worst[0]} ({worst[1]} pairs, {worst[2]:.2f}mm)")
