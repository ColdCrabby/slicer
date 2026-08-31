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

<div class="doc-source">Rendered from <a href="${githubUrl}"><code>${source}</code></a> in the repository — edit it there.</div>

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

// The shared Cold Crabby design language (ColdCrabby/ui). The Angular app
// resolves it through Sass `includePaths`; the docs use the same idiom via
// Vite's `loadPaths` below, so both sites read one set of token files and
// cannot drift. The checkout is git-ignored and created by `ui`'s postinstall
// (`pnpm --filter slicer-ui vendor:ui` to refresh it by hand).
const uiStyles = path.join(repoRoot, "ui/vendor/coldcrabby-ui/src/styles");

// https://vitepress.dev/reference/site-config
export default withMermaid(
  defineConfig({
    title: "Cold Crabby",
    description:
      "Slice your 3D models anywhere — in your browser, on your desktop, on an iPad, or on your own server.",
    lastUpdated: true,
    cleanUrls: true,
    base: "/docs/",

    // Same typeface pairing the app loads in `ui/src/index.html` — Plus
    // Jakarta Sans over IBM Plex Mono, both variable so the theme's non-step
    // weights (462 / 536 / 614) render as intended.
    head: [
      ["link", { rel: "preconnect", href: "https://fonts.googleapis.com" }],
      [
        "link",
        {
          rel: "preconnect",
          href: "https://fonts.gstatic.com",
          crossorigin: "",
        },
      ],
      [
        "link",
        {
          rel: "stylesheet",
          href: "https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@200..800&family=IBM+Plex+Mono:wght@400;500;600&display=swap",
        },
      ],
    ],

    themeConfig: {
      search: {
        provider: "local",
        options: { detailedView: true },
      },

      nav: [
        { text: "Use it", link: "/use/", activeMatch: "/use/" },
        { text: "For teams", link: "/teams/", activeMatch: "/teams/" },
        { text: "Brand", link: "/brand", activeMatch: "/brand" },
        {
          text: "Contribute",
          activeMatch: "/(guide|architecture)/",
          items: [
            { text: "Project overview", link: "/guide/" },
            { text: "Architecture", link: "/guide/architecture" },
            { text: "Contributing", link: "/guide/contributing" },
            { text: "Module reference", link: "/architecture/core" },
          ],
        },
      ],

      sidebar: {
        "/use/": [
          {
            text: "Using Cold Crabby",
            items: [
              { text: "Getting started", link: "/use/" },
              { text: "The interface", link: "/use/interface" },
              { text: "The build plate", link: "/use/plate" },
              { text: "Print settings", link: "/use/settings" },
              { text: "Printers & profiles", link: "/use/profiles" },
              { text: "Reading the preview", link: "/use/preview" },
              { text: "Sending to your printer", link: "/use/printing" },
              { text: "Keyboard & gestures", link: "/use/shortcuts" },
              { text: "Troubleshooting", link: "/use/troubleshooting" },
            ],
          },
          {
            text: "More",
            items: [
              { text: "For teams & businesses", link: "/teams/" },
              { text: "What's new", link: "/guide/changelog" },
              { text: "Brand", link: "/brand" },
            ],
          },
        ],
        "/teams/": [
          {
            text: "For teams & businesses",
            items: [
              { text: "Overview", link: "/teams/" },
              { text: "Self-hosting", link: "/teams/self-host" },
              { text: "Configuration", link: "/teams/configuration" },
              { text: "Automation & the CLI", link: "/teams/automation" },
              { text: "Data, privacy & licensing", link: "/teams/data" },
            ],
          },
          {
            text: "More",
            items: [
              { text: "Using the app", link: "/use/" },
              { text: "Building from source", link: "/guide/building" },
              { text: "Brand", link: "/brand" },
            ],
          },
        ],
        "/guide/": [
          {
            text: "Contributing",
            items: [
              { text: "Project overview", link: "/guide/" },
              { text: "Setup", link: "/guide/setup" },
              { text: "Building from source", link: "/guide/building" },
              { text: "Development", link: "/guide/development" },
              { text: "Contributing", link: "/guide/contributing" },
              { text: "Releasing", link: "/guide/releasing" },
              { text: "Agents (AI)", link: "/guide/agents" },
            ],
          },
          {
            text: "Architecture",
            items: [
              { text: "Overview", link: "/guide/architecture" },
              { text: "Module reference", link: "/architecture/core" },
            ],
          },
          {
            text: "Front-end",
            items: [
              { text: "Angular UI", link: "/guide/ui" },
              { text: "Theme", link: "/guide/ui-theme" },
              { text: "Styles", link: "/guide/ui-src-styles" },
              {
                text: "3D viewer",
                link: "/guide/ui-src-app-components-viewer",
              },
            ],
          },
        ],
        "/architecture/": [
          {
            text: "Pipeline",
            items: [
              { text: "Slicing pipeline (core)", link: "/architecture/core" },
              { text: "Slicing algorithm", link: "/architecture/slicing" },
              { text: "Mesh", link: "/architecture/mesh" },
              { text: "Walls", link: "/architecture/walls" },
              { text: "Infill patterns", link: "/architecture/infill" },
              { text: "Adhesion", link: "/architecture/adhesion" },
              { text: "G-code", link: "/architecture/gcode" },
            ],
          },
          {
            text: "Scene & settings",
            items: [
              { text: "Scene engine (SSOT)", link: "/architecture/scene" },
              { text: "Auto-orientation", link: "/architecture/orient" },
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
          {
            text: "Back to",
            items: [
              { text: "Contributing", link: "/guide/" },
              { text: "Using the app", link: "/use/" },
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
      css: {
        preprocessorOptions: {
          scss: {
            // `loadPaths` is the modern-API spelling of the `includePaths`
            // that `ui/angular.json` uses, so `@use 'theme/light'` resolves
            // to the shared library in both builds. Vite 5 still defaults to
            // the deprecated legacy API, which would ignore it.
            api: "modern",
            loadPaths: [uiStyles],
          },
        },
      },
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
