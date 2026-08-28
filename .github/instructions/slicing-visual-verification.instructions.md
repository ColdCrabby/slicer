---
description: "Use when changing anything that alters sliced G-code geometry — walls, infill, surfaces, gap fill, bridges, adhesion, ordering — and when opening or updating a PR for such a change. Defines the visual verification contract: render the actual toolpaths as capsules at their true bead width, before vs. after, and attach that image to the PR so a human can check the result without re-deriving the math."
name: "Slicing Changes: Prove It With a Picture"
applyTo: "src/core/**, src/walls/**, src/infill/**, src/adhesion/**, src/gcode/**, tools/gcode-analysis/**"
---

# Slicing Changes — Prove It With a Picture

Slicing is computational geometry: Clipper2 booleans, offsets, medial axes,
winding rules, scanlines. A reviewer **cannot** check that kind of change by
reading a diff, and neither can the author. Numbers alone are not enough either
— they are summaries, and summaries hide things.

So: **every change that alters sliced geometry ships with a before/after render
of the real toolpaths, attached to the PR.** The picture is the artifact that
lets a human agree or disagree in five seconds.

This is not decoration. It is the verification step.

## The one rule that matters: draw beads at their true width

**Never verify with a centerline plot.** Centerlines lie.

Two beads whose centerlines sit 0.3 mm apart look like two tidy, separate lines.
Drawn at their real 0.4–0.56 mm widths they are visibly **the same material laid
down twice**. That is the difference between "looks fine" and a defect.

This is not hypothetical. On the 3DBenchy rear rail a gap-fill bead ran directly
underneath the top-surface fill — 6 mm² per layer of genuine double extrusion.
It was invisible in a centerline plot, and `overlap.py` *also* missed it because
its ¼-nozzle erosion is designed to strip expected boundary seams and it ate the
thin bead entirely. A true-width capsule render showed it instantly.

Corollary: **when a metric says "clean" but the part looks wrong, trust the
picture and go find out why the metric lied.** A metric that disagrees with the
geometry is a bug in the metric's applicability, not a clean bill of health.

## Workflow

### 1. Slice both states

Build and slice the parent commit and your HEAD with identical settings. Only
the code may differ.

```bash
printf '[slicing]\nwall_generator = "arachne"\n' > /tmp/ar.toml

git stash -u                                     # park work in progress
git checkout <parent> -- src/core/ src/walls/    # or check out the parent commit
cargo build && ./target/debug/slicer-engine slice -i 3DBenchy.stl \
    --config /tmp/ar.toml -o /tmp/before.gcode

git checkout HEAD -- src/core/ src/walls/ && git stash pop
cargo build && ./target/debug/slicer-engine slice -i 3DBenchy.stl \
    --config /tmp/ar.toml -o /tmp/after.gcode
```

Always confirm the tree is restored (`git status`) before moving on.

### 2. Render the diff

[`tools/gcode-analysis/beaddiff.py`](../../tools/gcode-analysis/beaddiff.py)
does the before/after capsule render, role-coloured, on a shared scale, and
counts isolated short paths (the "tiny extrude / splat" defect class) in each
title:

```bash
# whole layer, auto-fit
python3 tools/gcode-analysis/beaddiff.py /tmp/before.gcode /tmp/after.gcode 41 /tmp/diff.png

# zoom a feature: cx cy half-window, flag anything under 1.5 mm
python3 tools/gcode-analysis/beaddiff.py /tmp/before.gcode /tmp/after.gcode \
    201 /tmp/rail.png 0.9 -12 2.2 --short=1.5
```

Pick the layer that shows the defect, not a layer that shows nothing. If the
change is meant to be a no-op somewhere, render that too — an unchanged panel is
evidence.

Related tools, all in [`tools/gcode-analysis/`](../../tools/gcode-analysis/README.md):
`zoom.py` (single-file capsules), `voids.py` (unfilled wall-zone gaps),
`overlap.py` (cross-role double extrusion), `widthdist.py`,
`coincident.py`. `--debug-geometry <dir>` dumps the *regions* (interior, solid
surface, infill) as SVG when you need to see what the booleans produced rather
than what was printed.

### 3. Pair the picture with numbers, and with `classic`

The image shows *what* changed; the numbers show it generalises and costs
little. Report both, and compare against the **`classic` wall generator** — the
trusted reference. "Better than before" is weak; "and still well inside what
classic does" is an argument.

Measure across several models (`3DBenchy.stl`, `Voron_Design_Cube_v7.stl`,
`Filament_Card_Caddy_25.stl`, `bottom_panel_hinge_x2.stl`) so the fix is not
tuned to one shape, and state explicitly that no model got worse. Always report
the **cost** (material or path length lost) next to the win.

### 4. Run the slicing quality gate — locally, before pushing

**This is not optional and it is easy to forget:** the gate is `#[ignore]`d, so
a plain `cargo test` skips it and reports all-green on a change that CI will
reject.

```bash
QA_FULL=1 cargo test --test slicing_quality -- --ignored
```

It slices the whole fixture corpus with both generators and fails on any metric
drifting >5 % from `tests/qa/baselines/*.json`. It exists precisely to catch the
model you *didn't* look at.

It works. A morphological opening of the infill area, validated by eye and by
metrics on the Benchy, looked like a clean win — and the gate caught that it had
erased 35 % of the filament caddy's infill (`caddy/classic: role infill
13170.1 -> 8505.5`), a fixture whose thin hollow-box lattice the change could
not distinguish from an artifact.

When it fails, **read the failing fixture before touching the baselines**:

- A **`classic`** delta from a change meant to affect only Arachne is a red flag
  — investigate, don't rebaseline.
- Reproduce the flagged case, render it, and confirm the new output is
  genuinely better. If it is not, the *fix* is wrong, not the baseline.
- Only when a delta is the intended, verified effect, refresh with
  `QA_FULL=1 UPDATE_QA_BASELINES=1 cargo test --test slicing_quality -- --ignored`,
  then **diff `tests/qa/baselines/` and justify every line** in the commit
  message. An unexplained baseline movement is a silent regression.

### 5. Attach it to the PR

Upload the PNG to GitHub's user-attachments API and embed the returned URL. Keep
untrusted values in quoted shell variables and let `--url-query` encode them.

```bash
FILE='/tmp/diff.png'; NAME='benchy-layer41-before-after.png'; MIME='image/png'
REPO='<owner>/<repo>'
REPO_ID="$(gh api "repos/$REPO" --jq .id)"
URL="$(curl --fail-with-body -sS -X POST \
  "https://uploads.github.com/user-attachments/assets" \
  --url-query "name=$NAME" --url-query "content_type=$MIME" \
  --url-query "repository_id=$REPO_ID" \
  -H "Content-Type: application/octet-stream" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  -H "Authorization: token $(gh auth token)" \
  --data-binary "@$FILE" | jq -r .url)"
case "$URL" in https://*) ;; *) echo "upload failed: $URL" >&2; exit 1;; esac

gh pr comment <N> --repo "$REPO" --body "![before/after]($URL)"
```

Verify a `https://` URL came back before embedding — a failed upload must fail
the step, not silently post a broken image. For a follow-up fix on an existing
PR, post a **comment** rather than rewriting the description, so the
investigation reads in order.

Caption every image. State what the reader should look at ("green gap-fill bead
runs under the red top surface"), not just what it is.

## Writing it up

Structure the PR body / comment as: **symptom → root cause → fix → evidence.**

- **Symptom** in the user's words where possible.
- **Root cause** — the actual mechanism, named concretely (which boolean, which
  region, which winding). "Fixed infill bug" is not a root cause.
- **Fix** — and why *this* fix rather than the obvious one. If you attacked a
  cause instead of a symptom, say so; that is the reviewable decision.
- **Evidence** — the image, a small table of numbers, and the `classic`
  comparison.

Prefer removing the cause over filtering the symptom, and say which you did. A
minimum-length filter that hides short artifacts leaves the *long* artifacts
from the same broken region in place; erasing the region fixes both.

## Gotchas that have burned this repo

- **Parsing G-code:** a move only extrudes if E **increased** *and* X/Y changed.
  Retract, un-retract, prime and Z-hop all change E without moving — counting
  them creates phantom zero-length paths and wrecks every statistic.
- **Path vs. segment:** a *path* is a contiguous run of extruding moves between
  travels. "Tiny extrude" defects are about **paths** (each costs a
  retract/travel/un-retract); per-segment counts are meaningless here because a
  long bead is made of many short segments.
- **Stale output.** Re-slice after *every* rebuild and confirm the binary is
  current. More than one wrong conclusion here came from measuring a `.gcode`
  produced by the previous build.
- **Ring vs. hole.** A region that should be solid but prints as two bands is
  usually a CCW outer contour with a **CW hole** punched in it. Check
  `signed_area()` per sub-path in the `--debug-geometry` SVG — a negative area
  is a hole. That single check found the rail-roof defect.
- **Debug SVG flips Y.** Negate Y when comparing SVG coordinates to G-code.
- **Layer indexing.** These scripts are 1-based over `;LAYER_CHANGE`; the UI
  viewer may number differently. Confirm via `;Z:` before claiming a layer
  number, and quote the Z height alongside it.
- **A thin channel is not automatically an artifact.** A sliver left by
  subtracting a *solid region* is one; a thin wall-to-wall cavity in a hollow
  box is a real feature whose lattice must survive. Key such a correction to the
  thing that *caused* the sliver (here, `solid_regions`) so it is a provable
  no-op elsewhere — never to the infill area as a whole.
- **`git stash` in a shared worktree.** Check `git stash list` first: a `pop`
  can restore *someone else's* older stash if your own `stash -u` found nothing
  to save. Prefer `git checkout <ref> -- <paths>` for temporary A/B builds, and
  verify `git status` afterwards.
- **Do not name a helper module `gc.py`** — it shadows Python's built-in `gc`
  and the import fails confusingly.

## See also

- [`tools/gcode-analysis/README.md`](../../tools/gcode-analysis/README.md) — the full toolkit.
- `AGENTS.md` → "Slicing Pipeline — Deep Knowledge" — the invariants a geometry
  change must not break. **Update it** when a change teaches something new.
