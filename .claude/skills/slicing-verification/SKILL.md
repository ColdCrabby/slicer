---
name: slicing-verification
description: Verify a change that alters sliced geometry by generating before/after pictures of real bead geometry and attaching them to the PR. Use when editing src/core, src/walls, src/infill, src/adhesion or src/gcode, or when the user asks how to prove a slicing change is correct, mentions wall overlap, gap fill, voids, bead width, or asks for a before/after render.
---

# Verifying a Slicing Change

**If geometry changed, the PR needs a before/after picture.** The full procedure
— slicing both revisions with identical settings, rendering, and attaching the
images — is in
[`slicing-visual-verification`](../../../.github/instructions/slicing-visual-verification.instructions.md).
Read it before starting; the steps below are the shape of it, not a substitute.

1. **Slice before and after with identical settings.** Only the code may differ.
   Check out the parent revision's slicing directories, build, slice; then
   restore and repeat.
2. **Render both** with [`tools/gcode-analysis/`](../../../tools/gcode-analysis/README.md).
3. **Attach the images to the PR.**

## Measure, do not assert

[`tools/gcode-analysis/`](../../../tools/gcode-analysis/README.md) measures
sliced G-code directly:

| Script | Answers |
| --- | --- |
| `coincident.py` | Where do beads overlap each other? |
| `voids.py` | What is left unfilled in the wall zone? |
| `widthdist.py` | Length-weighted bead-width distribution |
| `render.py` · `zoom.py` | Capsule and gap renders |

**Compare against the `classic` generator**, the trusted reference, before
claiming a fix. A defect `classic` also shows is not an Arachne bug.

Two traps that have cost real time here:

- **Use a true-width capsule intersection, not a footprint-erosion overlap
  scan.** A thin bead hides from the latter — that is how 6 mm²/layer of
  double-extrusion on the 3DBenchy rear rail went unseen.
- **Gap-fill length is not bit-reproducible** between runs of the same binary.
  Small gap-fill deltas are noise, not evidence. Sparse infill *is*
  deterministic and can be compared directly.
- **Never judge "did my change alter the output?" on a 3DBenchy.** The whole
  file moves — three consecutive slices of the *same binary* reported 3924.69,
  3924.87 and 3924.86 mm of filament, and `diff` says they differ. A refactor
  measured that way looks like a regression when it is noise, and a real
  regression smaller than that spread looks clean. Byte-compare a
  **deterministic fixture** instead, skipping the timestamp header:
  `Voron_Design_Cube_v7.stl`, `bottom_panel_hinge_x2.stl` and
  `Filament_Card_Caddy_25.stl` all reproduce exactly. **A passing quality gate
  is not evidence that output is unchanged** — its tolerances exist to absorb
  the Benchy's jitter.

## Then run the quality gate

The QA baselines exist to catch exactly the changes that look fine in one render
and destroy something elsewhere — a morphological-opening attempt once erased the
filament caddy's whole infill lattice, and the gate is what caught it. A baseline
that moves is a result to explain, never a number to re-record.

The pipeline invariants these checks defend are in
[`src/core/README.md`](../../../src/core/README.md).
