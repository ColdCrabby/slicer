---
description: "Use when a code change alters sliced geometry. Keep verification simple: generate before/after images of real bead geometry and attach them to the PR."
name: "Slicing Changes: Picture-First Verification"
applyTo: "src/core/**, src/walls/**, src/infill/**, src/adhesion/**, src/gcode/**, tools/gcode-analysis/**"
---

# Slicing Changes: Picture-First Verification

If geometry changes, always show a **before/after picture** in the PR.

## 1) Slice before and after with identical settings

Only the code may differ.

```bash
printf '[slicing]\nwall_generator = "arachne"\n' > /tmp/ar.toml

# before
git checkout <parent> -- src/core/ src/walls/ src/infill/ src/adhesion/ src/gcode/
cargo build
./target/debug/slicer-engine slice -i 3DBenchy.stl --config /tmp/ar.toml -o /tmp/before.gcode

# after
git checkout HEAD -- src/core/ src/walls/ src/infill/ src/adhesion/ src/gcode/
cargo build
./target/debug/slicer-engine slice -i 3DBenchy.stl --config /tmp/ar.toml -o /tmp/after.gcode
```

## 2) Render real beads (not centerlines)

Use the existing tool:

```bash
# whole layer
python3 tools/gcode-analysis/beaddiff.py /tmp/before.gcode /tmp/after.gcode 41 /tmp/diff.png

# zoomed area
python3 tools/gcode-analysis/beaddiff.py /tmp/before.gcode /tmp/after.gcode \
  201 /tmp/zoom.png 0.9 -12 2.2 --short=1.5
```

Pick the layer/zoom where the change is visible.

## 3) Attach image(s) to the PR

Upload and post:

```bash
FILE='/tmp/diff.png'; NAME='before-after.png'; MIME='image/png'
REPO='<owner>/<repo>'
REPO_ID="$(gh api "repos/$REPO" --jq .id)"
URL="$(curl --fail-with-body -sS -X POST \
  "https://uploads.github.com/user-attachments/assets" \
  --url-query "name=$NAME" \
  --url-query "content_type=$MIME" \
  --url-query "repository_id=$REPO_ID" \
  -H "Content-Type: application/octet-stream" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  -H "Authorization: token $(gh auth token)" \
  --data-binary "@$FILE" | jq -r .url)"
gh pr comment <N> --repo "$REPO" --body "![before/after]($URL)"
```

Add one short caption: what to look at.

## Optional helpers

- `tools/gcode-analysis/zoom.py` for single-file closeups.
- `tools/gcode-analysis/voids.py` and `tools/gcode-analysis/overlap.py` for extra numeric checks.
