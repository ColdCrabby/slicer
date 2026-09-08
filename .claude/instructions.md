# Slicer Engine - AI Agent Instructions

A high-performance 3D model slicer engine written in Rust, powered by [Clipper2](https://github.com/AngusJohnson/Clipper2) for polygon clipping operations.

These instructions apply to all AI agents (Claude, OpenAI, Anthropic, or any other) working on this codebase.

## Quick Reference

### Build & Run
```bash
cargo build                          # Debug build
cargo build --release               # Release build
cargo run                           # Run with default args
pnpm run dev                        # Start dev server (random seed)
pnpm run test                       # Run all tests
```

### Key Commands
```bash
cargo test                          # Unit & integration tests
cargo fmt && cargo clippy           # Format & lint
make build-release                  # Cross-platform builds
wasm-pack build --target web        # WebAssembly build
```

### Dev Server Details
- **Ports:** Dynamically assigned based on seed (4xxx for UI, 5xxx for engine)
- **Seed:** Use `--seed` to pin a specific port, `--print` to show without starting
- **No hardcoded ports:** The engine's port is internal — always use the dev server proxy

## Codebase Overview

### Primary Reference
**Read [`AGENTS.md`](../AGENTS.md) first** — it's the authoritative source for:
- Slicing pipeline orchestration and invariants
- Clipper2 fill rules and polygon winding requirements
- Module contracts and responsibilities
- Deep architectural knowledge
- Algorithm references and design decisions
- Known issues and edge cases

### Supporting Documents
- **[ARCHITECTURE.md](../ARCHITECTURE.md)** — High-level design and component relationships
- **[DEVELOPMENT.md](../DEVELOPMENT.md)** — Development workflow and common tasks
- **Module READMEs** — Each `src/*/` directory has a README for detailed context

### Core Modules
| Module | Purpose |
| --- | --- |
| `src/core/` | Clipper2 integration, polygon operations |
| `src/arachne/` | Variable-width wall algorithm |
| `src/infill/` | Infill pattern generation |
| `src/scene/` | Model loading and mesh processing |
| `src/gcode/` | G-code generation and optimization |

### Technology Stack
- **Language:** Rust (with WASM bindings)
- **Geometry:** Clipper2 (polygon clipping), nalgebra (linear algebra)
- **Frontend:** TypeScript/Angular
- **Build:** Cargo, pnpm, Makefile

## Agent Responsibilities

### When to Reach Out to Specific Agents

| Agent | Use When | Example |
| --- | --- | --- |
| **Senior Slicer Engineer** | Reviewing algorithms, geometry, architecture, numerical precision | "Is this wall offset calculation correct?" |
| **Documentation Sync** | Updating docs, keeping them aligned with code | "Update AGENTS.md for the new infill module" |
| **General/Claude** | Implementation, testing, refactoring, general questions | "Add this feature" or "Fix this bug" |

## General Guidelines for All Agents

### Before Starting Work
1. **Read the relevant AGENTS.md section** — it contains critical constraints and invariants
2. **Check module READMEs** — understand the contract before changing a module
3. **Verify git status** — ensure you're on the correct branch and state
4. **Run tests first** — baseline before changes to verify they pass

### During Implementation
- **Match existing patterns** — use the same style, naming, and structure as the codebase
- **Keep functions small** — prefer focused, testable units
- **Document non-obvious decisions** — explain the "why", not the "what"
- **Respect invariants** — if AGENTS.md says "X must be CCW", keep it that way

### Before Submitting
- **Run tests:** `cargo test`
- **Format & lint:** `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`
- **Build all targets:** `make build-release` or `wasm-pack build`
- **Manual testing:** Run `pnpm run dev` and verify the UI works

### Common Pitfalls to Avoid
- **Hardcoding ports** — use the dynamic seed system, never a fixed port
- **Ignoring fill rules** — Clipper2 requires correct EvenOdd/Positive/NonZero specification
- **Changing CCW winding** — holes must stay CW for Clipper2 operations
- **Missing edge cases** — consider degenerate polygons, zero-area islands, non-manifold mesh
- **Performance regressions** — profile hot paths (slicing loop, infill, wall offset) before optimizing

## Development Workflow

### Creating a Feature Branch
```bash
git checkout -b feature/your-feature-name
# Make changes
git add .
git commit -m "feat: description of changes"
git push origin feature/your-feature-name
```

### Running Specific Tests
```bash
cargo test --lib module_name          # Test one module
cargo test --test integration_test    # Test specific integration suite
cargo test -- --nocapture             # Show println! output
```

### Profiling and Benchmarking
- Use `cargo flamegraph` for CPU profiling (install: `cargo install flamegraph`)
- Use `cargo bench` for micro-benchmarks in `benches/`
- Check `DEVELOPMENT.md` for platform-specific tools

## Documentation Standards

### Code Comments
- Explain the **why**, not the **what**
- Keep comments short — one line when possible
- Add a comment only if removing it would confuse a reader
- Never document what the function name already says

### Doc Comments (on public APIs)
- One-line summary (fits in sidebar)
- Longer explanation if non-obvious
- Include examples for complex functions
- Link to related items with backticks

### Markdown Files
- Follow Diátaxis framework: Tutorial, How-to, Reference, Explanation
- User docs are plain English; dev docs are technical
- Lead with motivation → contract → anatomy
- Use tables for catalogs, small focused diagrams for flows

## Troubleshooting

### Build Fails with "wasm-bindgen schema mismatch"
- Run `cargo update` to sync dependencies
- Check CI logs to see if a wasm-bindgen version bump is needed
- See recent commits mentioning wasm-bindgen for context

### Tests Pass Locally but Fail in CI
- Ensure you built in release mode: `cargo build --release`
- Check for hard-coded paths or environment assumptions
- Run `cargo test --release` to match CI exactly

### Dev Server Won't Start
- Check if port is already in use: `lsof -i :4xxx`
- Try `pnpm run dev --seed 500` to pin a specific seed
- Verify Node version: `.nvmrc` or `.node-version` specifies required version

## Resources

- **AGENTS.md** — Slicing pipeline, fill rules, algorithm references, known issues
- **GitHub Issues** — Search for related problems; many have been solved
- **Tests** — Look at `src/tests/` and `tests/` for real examples of usage
- **External docs** — [Clipper2](https://github.com/AngusJohnson/Clipper2), [nalgebra](https://nalgebra.org/)

## Key Constraints

These are load-bearing — changing them breaks assumptions throughout the codebase:

1. **Winding order:** Outer contours are CCW, holes are CW (required by Clipper2)
2. **Coordinate system:** Integer coordinates in centimeter-scale units (for precision)
3. **Fill rules:** Always specify fill rule for Clipper2 operations; see AGENTS.md table
4. **Layer independence:** Each layer is processed independently; no cross-layer state
5. **Determinism:** Algorithms must be deterministic — never use random without seeding
6. **Memory safety:** Rust prevents most issues; use `unsafe` only if thoroughly justified

---

For agent-specific guidance (algorithms, documentation, implementation style), see the corresponding `.github/agents/*.agent.md` files.
