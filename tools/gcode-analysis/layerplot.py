#!/usr/bin/env python3
"""Draw one layer's *model cross-section* together with the beads laid into it.

Every other script here reads G-code, so it can only show what was printed. This
one answers the question G-code cannot: **how much of the material did the beads
actually cover?** — which is what you need to see a rib that stops short of its
own tip, or a thin feature that got no bead at all.

Input is the JSON that `cargo test --test dump_layer -- --ignored` writes; see
the README.

    layerplot.py <layer.json> <out.png> [cx cy half]
"""

import json
import math
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402

ROLE_COLOUR = {"OuterWall": "#444444", "InnerWall": "#5577dd", "GapFill": "#22aa44"}
FEATURE_COLOUR = "#ee2222"


def main() -> None:
    if len(sys.argv) < 3:
        print(__doc__)
        raise SystemExit(2)
    data = json.load(open(sys.argv[1]))
    out = sys.argv[2]
    window = [float(v) for v in sys.argv[3:6]] if len(sys.argv) > 5 else None

    fig, ax = plt.subplots(figsize=(11, 11), dpi=140)

    # The model's own cross-section, as outlines — a fill cannot show holes.
    for contour in data["island"]:
        a = np.array(contour)
        ax.plot(
            np.append(a[:, 0], a[0, 0]),
            np.append(a[:, 1], a[0, 1]),
            color="#000000",
            lw=1.1,
            zorder=5,
        )

    # Beads at their true width, so what they cover is what you see.
    for bead in data["beads"]:
        pts = np.array(bead["pts"])
        widths = np.array(bead["w"])
        colour = (
            FEATURE_COLOUR
            if bead["medial"] and bead["role"] == "OuterWall"
            else ROLE_COLOUR.get(bead["role"], "#999999")
        )
        for i in range(len(pts) - 1):
            w = (widths[min(i, len(widths) - 1)] + widths[min(i + 1, len(widths) - 1)]) / 2
            ax.plot(
                pts[i : i + 2, 0],
                pts[i : i + 2, 1],
                color=colour,
                lw=w * 140 / 25.4 * 0.5,
                solid_capstyle="round",
                zorder=3,
                alpha=0.85,
            )

    if window:
        cx, cy, half = window
        ax.set_xlim(cx - half, cx + half)
        ax.set_ylim(cy - half, cy + half)
    ax.set_aspect("equal")
    ax.grid(alpha=0.25)
    ax.set_title("black = model cross-section · red = feature bead · green = gap fill")
    fig.savefig(out, bbox_inches="tight")
    print("wrote", out)


if __name__ == "__main__":
    main()
