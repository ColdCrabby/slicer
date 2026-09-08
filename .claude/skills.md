# Available Skills & Tools for AI Agents

This document describes the tools and capabilities available to all AI agents working on the Slicer Engine codebase.

## Standard Tool Access

All agents have access to these core operations:

### Code Operations
- **read** — Read files and understand code structure
- **search** — Find code by pattern, symbol, or keyword
- **edit** — Modify existing source files
- **execute** — Run shell commands (within permission bounds)

### Project Management
- **todo** — Create and track tasks within the codebase

## Available Skills

### Project-Specific Skills

#### 1. **Senior Slicer Engineer**
**File:** `.github/agents/slicer-engineer.agent.md`

**Use when:**
- Reviewing slicing algorithms and computational geometry
- Designing pipeline stages and analyzing correctness
- Evaluating polygon clipping/offsetting
- Debugging numerical precision issues
- Comparing approaches to OrcaSlicer/PrusaSlicer/CuraEngine
- Performance-optimizing hot paths

**What it does:**
- Deep algorithmic review with assurance percentages
- Guidance meter for architecture quality, cleanliness, and performance
- Catches numerical edge cases and determinism issues
- Cross-references with mature slicer implementations

**Example invocation:**
```
@senior-slicer-engineer Is this wall offset calculation correct? 
It uses the Arachne algorithm but I'm concerned about the edge case handling.
```

---

#### 2. **Documentation Sync**
**File:** `.github/agents/docs-sync.agent.md`

**Use when:**
- Updating documentation and keeping it aligned with code
- Detecting outdated or missing docs
- Writing user guides or feature documentation
- Writing module READMEs or architecture explanations
- Auditing documentation quality against Diátaxis framework
- Improving AGENTS.md or other reference docs

**What it does:**
- Ensures docs match actual code behavior
- Follows Diátaxis framework (Tutorial, How-to, Reference, Explanation)
- Targets end-users and developers appropriately
- Validates that examples are correct and complete

**Example invocation:**
```
@docs-sync The infill module was refactored — update its README and 
add it to AGENTS.md. Make sure the examples still work.
```

---

#### 3. **ThreeJS 3D Engineer**
**File:** `.github/agents/threejs-3d-engineer.agent.md`

**Use when:**
- Working on 3D visualization, mesh rendering, or camera controls
- Debugging display issues or performance in the web UI
- Implementing interactive 3D features
- Adding annotations or overlays to the 3D view
- Optimizing WebGL or Three.js rendering

**What it does:**
- Specialized expertise in Three.js, WebGL, and 3D scene management
- Performance optimization for web-based 3D
- Guidance on camera systems, lighting, and material handling

**Example invocation:**
```
@threejs-3d-engineer The model rendering is slow with large meshes.
Can you profile and optimize the Three.js rendering pipeline?
```

---

## Tool Permissions

### Bash (Shell Execution)
Permitted operations (by pattern):
```
git checkout *      — Switch branches
git fetch *         — Update remote tracking branches
git reset *         — Undo commits locally
git merge *         — Merge branches
git rebase *        — Rebase history
git diff *          — Show changes
git log *           — View commit history
git status *        — Show working tree status
git add *           — Stage changes
git commit *        — Create commits
git push *          — Push to remote

pnpm install *      — Install dependencies
pnpm run *          — Run npm scripts
npm run *           — Run npm scripts
cargo *             — Build and test Rust code
make *              — Run Makefile targets

find *              — Search for files
grep *              — Search file contents
ls *                — List directories
cat *               — Display file contents
head *              — Show first lines
tail *              — Show last lines
wc *                — Count lines/words
```

### File Operations
- **Create new files** — Write new `.rs`, `.ts`, `.md`, `.json` files
- **Edit existing files** — Modify code, docs, configuration
- **Delete files** — Remove unused or obsolete files (with caution)

## How Agents Request Skills

When an agent needs specialized knowledge, it can request:

```yaml
---
name: "Custom Agent Name"
description: "When to use this agent"
tools: [read, search, edit, execute, todo]
model: "claude-opus-5"  # Optional; defaults to settings.json
argument-hint: "What the user should specify"
---

Agent instructions and expertise...
```

New agents can be added to `.github/agents/` following this format.

## Combining Skills

For complex tasks, chain multiple agents:

1. **Code implementation** — General/Claude agent
2. **Algorithm review** — Senior Slicer Engineer
3. **Documentation** — Documentation Sync
4. **Testing** — General/Claude agent

Example workflow:
```
1. Claude: "Add a new infill pattern feature"
2. Senior Slicer Engineer: "Review the algorithm correctness"
3. Documentation Sync: "Write the feature docs"
4. Claude: "Run tests and create a PR"
```

## Environment & Context

All agents operate within:
- **Repository root:** `/Users/max/Projects/slicer`
- **Worktree prefix:** `.claude/worktrees/[branch-name]`
- **Shared state:** Git repository, npm packages, Cargo dependencies
- **Time:** Use `git log` for temporal context; memory is session-local

## Best Practices for Agent Workflows

### Sequential Work
When one agent's output depends on another's:
```
1. Implement code changes (General Agent)
2. Verify algorithm correctness (Senior Slicer Engineer)
3. Run tests and fix failures (General Agent)
4. Document changes (Documentation Sync)
5. Create PR (General Agent)
```

### Parallel Work
When tasks are independent:
```
- Parallel 1: Implement feature in one module
- Parallel 2: Update docs for another area
- Parallel 3: Refactor tests
- Wait: Merge parallel work in one agent call
```

### Error Recovery
If an agent hits a blocker:
1. Check error message — is it a permission issue or a real blocker?
2. Switch to a different agent if needed
3. If permission issue, check `.claude/settings.json` allow-list
4. If code issue, fix and re-run the task

## Extending Skills

### Adding a New Agent
1. Create `.github/agents/your-agent-name.agent.md`
2. Follow the YAML frontmatter format (name, description, tools, model)
3. Write detailed instructions and constraints
4. Reference this skills doc from your agent
5. Update `.claude/settings.json` if new permissions needed

### Updating Tool Access
Edit `.claude/settings.json` → `permissions.allow` to add new patterns.

---

**Master skill reference:** See individual `.github/agents/*.agent.md` files for detailed expertise and constraints.
