/// <reference types="vite/client" />
import { dirname, resolve } from 'path';
import { fileURLToPath } from 'url';
import { defineConfig } from 'vite';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// pnpm hoists packages into node_modules/.pnpm/<pkg>@<version>/node_modules/<pkg>.
// Vite's default fs.allow list does not include that virtual store path, which
// causes "outside of Vite serving allow list" errors for packages like
// monaco-editor that load CSS/fonts (codicon.ttf, codicon-modifiers.css) via
// runtime URLs. Allowing the entire monorepo root covers both ui/node_modules
// symlinks and their real targets under <repo>/node_modules/.pnpm/*.
export default defineConfig({
  // The code editor imports Monaco's *modular* entry points rather than the
  // package root (see ui/src/app/components/code-editor/code-editor.ts for why).
  // Vite pre-bundles dependencies it finds by crawling static imports, but these
  // are reached through a lazy `import()` inside a lazily-routed component, so it
  // only discovers them the first time an editor mounts — mid-session, after the
  // page has loaded. Re-optimising then invalidates the module graph the tab is
  // already running against, and the fetch that triggered it fails with
  // "504 (Outdated Optimize Dep)": the editor silently never appears.
  //
  // Naming them here makes them part of the initial pre-bundle, so the specifier
  // is stable from the first request. Keep this list in step with the dynamic
  // imports in code-editor.ts.
  optimizeDeps: {
    include: [
      'monaco-editor/editor/editor.api',
      'monaco-editor/features/register.all',
      'monaco-editor/language/json/monaco.contribution',
    ],
  },
  server: {
    fs: {
      // Monaco's ESM runtime can resolve CSS asset requests to absolute
      // realpaths under pnpm's virtual store. Keep fs checks enabled but allow
      // those paths explicitly.
      strict: false,
      allow: [
        // UI workspace root.
        __dirname,
        // Monorepo root — covers ui/node_modules and the pnpm virtual
        // store at <repo>/node_modules/.pnpm/*
        resolve(__dirname, '..'),
        // Explicit pnpm virtual store path used by realpath resolution.
        resolve(__dirname, '../node_modules/.pnpm'),
      ],
    },
  },
});
