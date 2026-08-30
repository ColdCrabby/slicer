#!/usr/bin/env python3
"""Regenerate the known-bad STL corpus used by `tests/mesh_repair.rs`.

Every fixture is a 10 mm cube carrying exactly one class of defect, so a test
failure points at a single repair step. Run from the repository root:

    python3 tests/fixtures/broken/generate.py

The output is deterministic — re-running it must not produce a diff.
"""

from __future__ import annotations

import struct
from pathlib import Path

S = 10.0
OUT = Path(__file__).parent

# Cube corners: 0-3 bottom (z=0) counter-clockwise, 4-7 top (z=S).
P = [
    (0.0, 0.0, 0.0),
    (S, 0.0, 0.0),
    (S, S, 0.0),
    (0.0, S, 0.0),
    (0.0, 0.0, S),
    (S, 0.0, S),
    (S, S, S),
    (0.0, S, S),
]

# Each quad is wound so its normal points out of the cube.
QUADS = [
    (0, 3, 2, 1),  # bottom  -Z
    (4, 5, 6, 7),  # top     +Z
    (0, 1, 5, 4),  # front   -Y
    (1, 2, 6, 5),  # right   +X
    (2, 3, 7, 6),  # back    +Y
    (3, 0, 4, 7),  # left    -X
]


def cube() -> list[tuple[tuple[float, float, float], ...]]:
    tris = []
    for a, b, c, d in QUADS:
        tris.append((P[a], P[b], P[c]))
        tris.append((P[a], P[c], P[d]))
    return tris


def normal(tri):
    (ax, ay, az), (bx, by, bz), (cx, cy, cz) = tri
    ux, uy, uz = bx - ax, by - ay, bz - az
    vx, vy, vz = cx - ax, cy - ay, cz - az
    nx, ny, nz = uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx
    length = (nx * nx + ny * ny + nz * nz) ** 0.5
    if length == 0.0:
        return (0.0, 0.0, 0.0)
    return (nx / length, ny / length, nz / length)


def write(name: str, tris) -> None:
    header = f"{name} - deliberately defective test fixture".encode()
    data = bytearray(header.ljust(80, b"\0"))
    data += struct.pack("<I", len(tris))
    for tri in tris:
        data += struct.pack("<3f", *normal(tri))
        for vertex in tri:
            data += struct.pack("<3f", *vertex)
        data += struct.pack("<H", 0)
    (OUT / name).write_bytes(bytes(data))
    print(f"{name}: {len(tris)} triangles")


def flip(tri):
    return (tri[0], tri[2], tri[1])


def main() -> None:
    # A 4-edge hole: both triangles of the top face are missing.
    tris = cube()
    write("cube-hole.stl", tris[:2] + tris[4:])

    # One triangle wound against its neighbours.
    tris = cube()
    tris[0] = flip(tris[0])
    write("cube-flipped-face.stl", tris)

    # Every triangle wound inward — the whole shell is inside out.
    write("cube-inverted.stl", [flip(t) for t in cube()])

    # Zero-area triangles: three collinear points, and a repeated corner.
    tris = cube()
    tris.append(((1.0, 1.0, 0.0), (2.0, 2.0, 0.0), (3.0, 3.0, 0.0)))
    tris.append(((4.0, 4.0, 0.0), (4.0, 4.0, 0.0), (5.0, 5.0, 0.0)))
    write("cube-degenerate.stl", tris)

    # The first triangle, written twice.
    tris = cube()
    tris.insert(1, tris[0])
    write("cube-duplicate-faces.stl", tris)

    # A crack: the bottom face uses a corner nudged 1e-5 mm away from the one
    # its neighbours use. Well inside the 1e-4 mm weld tolerance, and still
    # distinct after the f32 round-trip STL forces.
    cracked = (1e-5, 1e-5, 0.0)
    tris = []
    for index, tri in enumerate(cube()):
        if index < 2:  # the two bottom-face triangles
            tri = tuple(cracked if v == P[0] else v for v in tri)
        tris.append(tri)
    write("cube-unwelded.stl", tris)

    # A T-junction: the bottom face is split at the midpoint of one cube edge
    # while the neighbouring side face still spans the whole edge. No triangle
    # is degenerate, but the three half-edges around the join have only one
    # incident face each, so they read as a boundary loop — a *collinear* one,
    # enclosing zero area. This is a slit, not a hole: patching it could only
    # ever add zero-area triangles. Extremely common in exported STLs.
    tris = cube()
    p0, p1, p2 = P[0], P[1], P[2]
    mid = tuple((a + b) / 2 for a, b in zip(p0, p1))
    tris = [t for t in tris if t != (p0, p2, p1)]
    tris.append((p2, p1, mid))
    tris.append((p2, mid, p0))
    write("cube-tjunction.stl", tris)

    # Everything at once, to pin the interaction between the repair steps.
    tris = cube()
    tris[2] = flip(tris[2])
    tris.append(tris[4])
    tris.append(((1.0, 1.0, 0.0), (2.0, 2.0, 0.0), (3.0, 3.0, 0.0)))
    write("cube-multi-defect.stl", tris[:8] + tris[10:])


if __name__ == "__main__":
    main()
