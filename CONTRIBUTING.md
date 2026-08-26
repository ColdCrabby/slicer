# Contributing to Slicer Engine

Thank you for your interest in contributing to Slicer Engine! This guide will help you get started with the codebase and development workflow.

## Quick Start

1. **Fork the repository** on GitHub
2. **Clone your fork:**
   ```bash
   git clone https://github.com/YOUR-USERNAME/slicer-engine.git
   cd slicer-engine
   ```
3. **Build and test:**
   ```bash
   cargo build
   cargo test
   ```
4. **Read the architecture:** See [ARCHITECTURE.md](ARCHITECTURE.md) for a comprehensive overview

## Before You Start

### Essential Reading

- [ARCHITECTURE.md](ARCHITECTURE.md) — Complete system architecture with Mermaid diagrams
- [README.md](README.md) — Usage and quick start guide
- [src/SLICING.md](src/SLICING.md) — Slicing algorithm deep-dive

### Understanding the Codebase

The slicer is organized into focused modules:

```
src/
├── core.rs          → Main slicing pipeline (start here!)
├── arachne/         → Variable-width wall generation
├── mesh/            → STL loading and transformations
├── gcode/           → G-code emission (multi-flavor)
├── settings/        → Configuration and validation
├── cli/             → Command-line interface
└── server/          → WebSocket API for web UI
```

**Start by reading:** `src/core.rs` → `process_mesh()` function

## Development Workflow

### 1. Set Up Your Environment

**Install Rust:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Install development tools:**
```bash
rustup component add clippy rustfmt
```

**For WebAssembly builds:**
```bash
cargo install wasm-pack
```

**Set up the workspace + git hooks:**
```bash
pnpm install   # also installs Lefthook (via the `prepare` script)
```
Lefthook then autoformats your **staged** files on every commit — Prettier for the
Angular UI (`ui/**/*.{ts,html,scss,css}`) and `rustfmt` for Rust (`*.rs`) — so what you
commit already matches CI. Bypass once with `git commit --no-verify` or disable with
`LEFTHOOK=0 git commit`.

### 2. Create a Feature Branch

```bash
git checkout -b feature/your-feature-name
```

Use descriptive names:
- `feature/gyroid-infill-pattern`
- `fix/arachne-thin-wall-crash`
- `docs/explain-surface-detection`

### 3. Make Your Changes

**Code Style:**
- Follow Rust idioms and conventions
- Use `cargo fmt` to format code (enforced by CI; also applied automatically on commit by Lefthook)
- Run `cargo clippy` to catch common issues
- Add doc comments (`///`) for public APIs
- UI code (`ui/`) is formatted with Prettier — also applied on commit by Lefthook

**Testing:**
- Write tests for new functionality
- Use inline tests with `#[cfg(test)]` modules
- Run `cargo test` frequently during development
- Ensure `cargo test --release` passes before committing

**Example test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapse_depth_square() {
        let paths = square_paths(10.0);
        let d = find_collapse_depth(&paths);
        assert!((d - 5.0).abs() < 0.02, "Expected ~5mm, got {}", d);
    }
}
```

### 4. Commit Your Changes

> On commit, Lefthook autoformats your staged files (Prettier + `rustfmt`) and re-stages
> them, so formatting never blocks your PR. Use `git commit --no-verify` to skip it.

**Commit message format:**
```
type(scope): short description

Longer explanation if needed, wrapping at 72 characters.
Include motivation, context, and implementation notes.

Fixes #123
```

**Types:**
- `feat:` — New feature
- `fix:` — Bug fix
- `docs:` — Documentation changes
- `refactor:` — Code restructuring (no behavior change)
- `test:` — Adding or fixing tests
- `chore:` — Build config, dependencies, etc.

**Examples:**
```
feat(arachne): add wall_distribution_count parameter

Allows distributing residual width across multiple innermost beads
instead of just the last one. Improves finish on tapered walls.

Fixes #45

---

fix(gcode): correct extrusion calculation for variable widths

Was using nozzle_diameter_mm for all paths; now uses per-path
width from Arachne. Fixes under-extrusion on thin-wall beads.

Fixes #67
```

### 5. Push and Create a Pull Request

```bash
git push origin feature/your-feature-name
```

Then open a PR on GitHub with:
- **Clear title** describing the change
- **Description** explaining what and why
- **Link to issue** if fixing a bug or implementing a feature request
- **Screenshots/videos** if changing UI or output quality

## Code Quality Standards

### Must Pass Before Merge

```bash
# Format check
cargo fmt --check

# Linting (zero warnings)
cargo clippy --all-targets --all-features -- -D warnings

# All tests passing
cargo test --release

# No compiler warnings
cargo build --release 2>&1 | grep warning
```

### Optional But Recommended

```bash
# Check for unused dependencies
cargo install cargo-udeps
cargo +nightly udeps

# Security audit
cargo install cargo-audit
cargo audit

# Check for outdated dependencies
cargo install cargo-outdated
cargo outdated
```

## What to Contribute

### Good First Issues

Look for issues labeled `good first issue` on GitHub. These are typically:
- Documentation improvements
- Adding tests for existing code
- Small bug fixes with clear reproduction steps
- Parameter validation rules

### Ideas for Contributions

**Features:**
- New infill patterns (cubic, hilbert curve, lightning)
- Support structure generation
- Adaptive layer heights
- Ironing (smooth top surfaces)
- Sequential printing (object-by-object)
- Custom G-code insertion at layer changes

**Improvements:**
- Performance optimizations (profiling with `cargo flamegraph`)
- Better error messages
- More comprehensive tests
- Additional G-code flavors (RepRap, Prusa, etc.)
- UI enhancements

**Documentation:**
- Tutorial blog posts
- Video walkthroughs
- API documentation examples
- Translation to other languages

### Areas That Need Help

Check the [GitHub issues](https://github.com/max-scopp/slicer-engine/issues) for:
- `help wanted` — Community input desired
- `enhancement` — Feature requests
- `bug` — Known issues

## Development Tips

### Debugging

**Add trace logging:**
```rust
use crate::logging::ProcessLogger;

logger.log_debug(&format!("Collapse depth: {:.4}mm", depth));
```

**Visualize intermediate results:**
```rust
// Export Clipper2 paths as SVG for inspection
use clipper2::*;
let svg = paths.to_svg(1000, 1000);
std::fs::write("debug.svg", svg)?;
```

**Profile performance:**
```bash
cargo install flamegraph
cargo flamegraph --bin slicer-engine -- slice --input large.stl
# Opens flamegraph.svg in browser
```

### Testing Strategy

**Unit tests:** Test individual functions in isolation
```rust
#[test]
fn test_edge_intersect() {
    let a = Vertex { x: 0.0, y: 0.0, z: 1.0 };
    let b = Vertex { x: 10.0, y: 10.0, z: 3.0 };
    let (x, y) = edge_intersect(a, b, 2.5);
    assert_eq!(x, 7.5);
    assert_eq!(y, 7.5);
}
```

**Integration tests:** Test module interactions
```rust
#[test]
fn test_process_mesh_pipeline() {
    let mesh = create_test_cube(10.0);
    let params = SlicingParams::default();
    let layers = process_mesh(&mesh, &params, &NullLogger);
    assert!(!layers.is_empty());
}
```

**Property-based tests:** Use `proptest` or `quickcheck` for fuzzing
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_collapse_depth_positive(side in 1.0..100.0f64) {
        let paths = square_paths(side);
        let d = find_collapse_depth(&paths);
        assert!(d > 0.0 && d <= side / 2.0);
    }
}
```

### Common Pitfalls

**1. Clipper2 coordinate precision:**
- Uses integer-based `Centi` (centimeter precision)
- Be careful with floating-point conversions
- Use simplify() after inflate() to remove duplicate points

**2. Winding order:**
- Outer contours = counter-clockwise
- Holes = clockwise
- Use `FillRule::EvenOdd` when winding isn't guaranteed

**3. Path roles:**
- Always set `path_roles` when adding paths to a layer
- Length must match `paths.len()` or defaults to `Perimeter`

**4. Settings defaults:**
- Use `#[serde(default)]` for new fields to maintain backward compatibility
- Add default value functions: `fn default_my_param() -> T`

**5. Memory in loops:**
- Avoid cloning large structures in hot paths
- Use `&` references when possible
- Profile with `cargo flamegraph` if performance degrades

## Documentation Standards

### Code Comments

**Doc comments (`///`) for public APIs:**
```rust
/// Compute the collapse depth of a polygon.
///
/// The collapse depth D is the largest inward offset at which the polygon
/// is still non-empty. It equals the polygon's inradius (half the minimum
/// local wall thickness).
///
/// # Arguments
/// * `input` - The polygon paths to analyze
///
/// # Returns
/// The collapse depth in millimeters, or 0.0 if the polygon is degenerate.
///
/// # Example
/// ```
/// let square = square_paths(10.0);
/// let depth = find_collapse_depth(&square);
/// assert_eq!(depth, 5.0); // Half the side length
/// ```
pub fn find_collapse_depth(input: &Paths) -> f64 {
    // ...
}
```

**Inline comments for complex logic:**
```rust
// Strategy: binary search the collapse depth using Clipper2 inflate().
// 24 iterations give sub-nanometer precision on a 500mm bbox.
for _ in 0..COLLAPSE_DEPTH_ITERATIONS {
    let mid = (lo + hi) / 2.0;
    // ...
}
```

### Markdown Documentation

**Update existing docs when adding features:**
- `ARCHITECTURE.md` — High-level architecture changes
- `README.md` — User-facing feature descriptions
- Module READMEs — Implementation details

**Add Mermaid diagrams for visual clarity:**
```markdown
## Data Flow

\`\`\`mermaid
graph LR
    A[Input] --> B[Process]
    B --> C[Output]
\`\`\`
```

## Pull Request Checklist

Before submitting your PR, ensure:

- [ ] Code builds: `cargo build --release`
- [ ] All tests pass: `cargo test --release`
- [ ] Linting passes: `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] Code is formatted: `cargo fmt` (Lefthook applies this automatically on commit)
- [ ] New code has tests
- [ ] Public APIs have doc comments
- [ ] ARCHITECTURE.md updated if architecture changed
- [ ] README.md updated if user-facing features added
- [ ] Commit messages follow the format
- [ ] PR description explains what/why/how

## Getting Help

- **Questions:** Open a [GitHub Discussion](https://github.com/max-scopp/slicer-engine/discussions)
- **Bugs:** Open an [Issue](https://github.com/max-scopp/slicer-engine/issues)
- **Architecture:** Read [ARCHITECTURE.md](ARCHITECTURE.md)
- **Chat:** (Add Discord/Matrix/IRC if available)

## Code of Conduct

Be respectful, constructive, and inclusive. We're all here to learn and build something awesome together.

**Expected behavior:**
- Be welcoming to newcomers
- Provide constructive feedback on PRs
- Assume good intentions
- Focus on the code, not the person

**Unacceptable:**
- Harassment, trolling, or personal attacks
- Discriminatory language or behavior
- Spam or off-topic discussions

Violations will result in comment deletion, temporary bans, or permanent bans depending on severity.

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (see LICENSE file).

---

**Thank you for contributing!** Every contribution, no matter how small, helps make this project better. 🚀
