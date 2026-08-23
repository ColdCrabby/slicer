import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitepress";
import { withMermaid } from "vitepress-plugin-mermaid";

// README discovery: auto-generates thin wrapper pages from repo READMEs.
// Sidebar is defined manually below.

type DocPage = {
  source: string; // repo-root-relative path to the README
};

const docsRoot = fileURLToPath(new URL("..", import.meta.url));
const repoRoot = path.resolve(docsRoot, "..");

const IGNORED_DIRS = new Set([
  "node_modules",
  "target",
  "dist",
  ".angular",
  ".vitepress",
  ".git",
  "docs",
  "stls",
  "tests",
  "plan",
  "generated", // wasm-pack and codegen output — never docs source
]);

function findReadmes(dir: string, out: string[] = []): string[] {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (IGNORED_DIRS.has(entry.name) || entry.name.startsWith(".")) {
        continue;
      }
      findReadmes(path.join(dir, entry.name), out);
    } else if (entry.isFile() && /^README\.md$/i.test(entry.name)) {
      out.push(path.join(dir, entry.name));
    }
  }
  return out;
}

// Find non-README .md files under src/ and ui/ (e.g. SLICING.md, logging.md,
// THEME.md). Routed by writeWrapper using the same architecture-/guide-
// flattening rules as READMEs.
function findExtraDocs(dir: string, out: string[] = []): string[] {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (IGNORED_DIRS.has(entry.name) || entry.name.startsWith(".")) {
        continue;
      }
      findExtraDocs(path.join(dir, entry.name), out);
    } else if (
      entry.isFile() &&
      entry.name.endsWith(".md") &&
      !/^README\.md$/i.test(entry.name)
    ) {
      out.push(path.join(dir, entry.name));
    }
  }
  return out;
}

const discovered: Map<string, DocPage> = new Map();
for (const absPath of findReadmes(repoRoot)) {
  const rel = path.relative(repoRoot, absPath).replace(/\\/g, "/");
  discovered.set(rel, { source: rel });
}

// Also pull in stand-alone .md files inside src/ and ui/ (SLICING.md,
// logging.md, THEME.md, …).
for (const subtree of ["src", "ui"]) {
  const absSubtree = path.join(repoRoot, subtree);
  if (!fs.existsSync(absSubtree)) {
    continue;
  }
  for (const absPath of findExtraDocs(absSubtree)) {
    const rel = path.relative(repoRoot, absPath).replace(/\\/g, "/");
    discovered.set(rel, { source: rel });
  }
}

// Also pick up uppercase top-level docs (ARCHITECTURE.md, CONTRIBUTING.md,
// AGENTS.md, QUICK_REFERENCE.md, SETUP_COMPLETE.md) so they show up under
// the Guide section.
for (const entry of fs.readdirSync(repoRoot, { withFileTypes: true })) {
  if (!entry.isFile()) {
    continue;
  }
  if (entry.name === "README.md") {
    continue; // already discovered
  }
  if (!/^[A-Z_]+\.md$/.test(entry.name)) {
    continue;
  }
  discovered.set(entry.name, { source: entry.name });
}

// Auto-generate wrapper pages on every config load. Each wrapper is a
// one-line `<!--@include: ... -->` directive pointing at the real README, so
// the docs site stays a thin shell — never a copy.
function writeWrapper(source: string) {
  // Route source → URL using simple rules. Sidebar definitions must match.
  let url: string;
  if (source === "README.md") {
    url = "guide/index";
  } else if (/^[A-Z_]+\.md$/i.test(source)) {
    url = `guide/${source.replace(/\.md$/i, "").toLowerCase()}`;
  } else if (source.startsWith("src/") && source.endsWith("/README.md")) {
    const inner = source.slice("src/".length, -"/README.md".length);
    url = `architecture/${inner.replace(/\//g, "-")}`;
  } else if (source.startsWith("src/") && source.endsWith(".md")) {
    // Stand-alone src/ markdown (e.g. src/SLICING.md, src/logging.md).
    const inner = source.slice("src/".length, -".md".length);
    url = `architecture/${inner.replace(/\//g, "-").toLowerCase()}`;
  } else if (source.startsWith("ui/") && source.endsWith("/README.md")) {
    const inner = source.slice("ui/".length, -"/README.md".length);
    url = inner === "" ? "guide/ui" : `guide/ui-${inner.replace(/\//g, "-")}`;
  } else if (source.startsWith("ui/") && source.endsWith(".md")) {
    // Stand-alone ui/ markdown (e.g. ui/THEME.md).
    const inner = source.slice("ui/".length, -".md".length);
    url = `guide/ui-${inner.replace(/\//g, "-").toLowerCase()}`;
  } else {
    return; // Skip unroutable files
  }

  const wrapperPath = path.join(docsRoot, `${url}.md`);
  const includePath = path
    .relative(path.dirname(wrapperPath), path.join(repoRoot, source))
    .replace(/\\/g, "/");
  const githubUrl = `https://github.com/max-scopp/slicer-engine/blob/main/${source}`;
  const contents = `---
editLink: false
---

> **Source:** [\`${source}\`](${githubUrl}) — this page is rendered directly from the file in the repository. Edit it there.

<!--@include: ${includePath}-->
`;
  fs.mkdirSync(path.dirname(wrapperPath), { recursive: true });
  const existing = fs.existsSync(wrapperPath)
    ? fs.readFileSync(wrapperPath, "utf8")
    : null;
  if (existing !== contents) {
    fs.writeFileSync(wrapperPath, contents);
  }
}

for (const source of discovered.keys()) {
  writeWrapper(source);
}

// https://vitepress.dev/reference/site-config
export default withMermaid(
  defineConfig({
    title: "Slicer Engine",
    description:
      "A high-performance 3D model slicer engine written in Rust, powered by Clipper2.",
    lastUpdated: true,
    cleanUrls: true,
    base: "/docs/",

    themeConfig: {
      search: {
        provider: "local",
        options: { detailedView: true },
      },

      nav: [
        { text: "Guide", link: "/guide/", activeMatch: "/guide/" },
        {
          text: "Architecture",
          link: "/architecture/core",
          activeMatch: "/architecture/",
        },
      ],

      sidebar: {
        "/guide/": [
          {
            text: "Overview",
            items: [{ text: "Project Overview", link: "/guide/" }],
          },
          {
            text: "Working on the engine",
            items: [
              { text: "Architecture", link: "/guide/architecture" },
              { text: "Contributing", link: "/guide/contributing" },
              { text: "Releasing", link: "/guide/releasing" },
              { text: "Agents (AI)", link: "/guide/agents" },
            ],
          },
          {
            text: "UI",
            items: [
              { text: "Angular UI", link: "/guide/ui" },
              { text: "Theme", link: "/guide/ui-theme" },
              { text: "Styles", link: "/guide/ui-src-styles" },
            ],
          },
        ],
        "/architecture/": [
          {
            text: "Pipeline",
            items: [
              { text: "Slicing Pipeline (core)", link: "/architecture/core" },
              { text: "Slicing algorithm", link: "/architecture/slicing" },
              { text: "Mesh", link: "/architecture/mesh" },
              { text: "Arachne (walls)", link: "/architecture/arachne" },
              { text: "Infill patterns", link: "/architecture/infill" },
              { text: "G-code", link: "/architecture/gcode" },
            ],
          },
          {
            text: "Scene & Settings",
            items: [
              { text: "Scene Engine (SSOT)", link: "/architecture/scene" },
              { text: "Settings", link: "/architecture/settings" },
              { text: "Config (TOML)", link: "/architecture/config" },
            ],
          },
          {
            text: "Interfaces",
            items: [
              { text: "CLI", link: "/architecture/cli" },
              { text: "Server (HTTP + WS)", link: "/architecture/server" },
              { text: "Database (SQLite)", link: "/architecture/db" },
              { text: "Logging", link: "/architecture/logging" },
            ],
          },
        ],
      },

      socialLinks: [
        {
          icon: "github",
          link: "https://github.com/max-scopp/slicer-engine",
        },
      ],

      outline: { level: [2, 3] },
    },

    // Cross-links between READMEs use filesystem paths that are valid on
    // GitHub but don't match the rendered URLs. Skip the strict check
    // rather than touching upstream README content.
    ignoreDeadLinks: true,

    markdown: {
      lineNumbers: false,
      languages: [
        {
          name: "gcode",
          aliases: ["gc", "nc", "cnc"],
          scopeName: "source.gcode",
          path: path.resolve(docsRoot, ".vitepress/gcode.tmLanguage.json"),
        } as any,
      ],
    },

    vite: {
      optimizeDeps: {
        include: ["mermaid"],
      },
      ssr: {
        noExternal: ["vitepress-plugin-mermaid", "mermaid"],
      },
    },

    mermaid: {
      // Theme automatically follows the VitePress dark/light mode.
    },
  }),
);
