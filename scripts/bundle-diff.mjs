#!/usr/bin/env node
//
// bundle-diff.mjs — say which modules make up the UI's initial bundle, and
// which ones moved it.
//
// The `initial` budget in ui/angular.json gates the bytes the browser needs
// before the first paint. When it trips, Angular names hashed chunks, which
// says nothing about *why*. This reads the esbuild metafile that
// `ng build --stats-json` writes and attributes the initial bytes back to
// source files and npm packages — and, given a baseline, ranks what changed
// and names the importer that pulled each newcomer in.
//
// Usage:
//   node scripts/bundle-diff.mjs ui/dist/slicer-ui/browser-stats.json
//   node scripts/bundle-diff.mjs --base base/browser-stats.json ui/dist/slicer-ui/browser-stats.json
//
// Prints Markdown, so CI can append it to the job summary unchanged.

import { readFileSync } from 'node:fs';

const MIN_DELTA = 500; // bytes; smaller moves are noise from hashing and minification
const MAX_ROWS = 20;

const args = process.argv.slice(2);
const baseIdx = args.indexOf('--base');
const basePath = baseIdx >= 0 ? args.splice(baseIdx, 2)[1] : undefined;
const [headPath] = args;
if (!headPath) {
  console.error('usage: bundle-diff.mjs [--base <stats.json>] <stats.json>');
  process.exit(2);
}

const head = analyse(JSON.parse(readFileSync(headPath, 'utf8')));
const base = basePath ? analyse(JSON.parse(readFileSync(basePath, 'utf8'))) : undefined;

const kb = (bytes) => `${(bytes / 1000).toFixed(2)} kB`;
const signed = (bytes) => `${bytes >= 0 ? '+' : '-'}${kb(Math.abs(bytes))}`;
const out = ['### Initial bundle', ''];

if (!base) {
  out.push(`**${kb(head.total)}** — no baseline to compare against. Largest modules:`, '');
  out.push('| Size | Module |', '|--:|---|');
  for (const [mod, bytes] of [...head.modules].sort((a, b) => b[1] - a[1]).slice(0, MAX_ROWS)) {
    out.push(`| ${kb(bytes)} | \`${mod}\` |`);
  }
} else {
  out.push(
    `**${kb(head.total)}** — ${signed(head.total - base.total)} vs the base build (${kb(base.total)})`,
    '',
  );
  const changes = [...new Set([...head.modules.keys(), ...base.modules.keys()])]
    .map((mod) => [mod, (head.modules.get(mod) ?? 0) - (base.modules.get(mod) ?? 0)])
    .filter(([, delta]) => Math.abs(delta) >= MIN_DELTA)
    .sort((a, b) => Math.abs(b[1]) - Math.abs(a[1]));
  if (changes.length === 0) {
    out.push(`No module moved by ${kb(MIN_DELTA)} or more.`);
  } else {
    out.push('| Change | Size | Module | Pulled in by |', '|--:|--:|---|---|');
    for (const [mod, delta] of changes.slice(0, MAX_ROWS)) {
      // Only a newcomer's importer is news; for a module that merely grew it
      // would just list everything that already used it.
      const via = base.modules.has(mod) ? '' : head.importersOf(mod);
      out.push(`| ${signed(delta)} | ${kb(head.modules.get(mod) ?? 0)} | \`${mod}\` | ${via} |`);
    }
    if (changes.length > MAX_ROWS)
      out.push('', `…and ${changes.length - MAX_ROWS} smaller changes.`);
  }
}

out.push(
  '',
  "_Module sizes are esbuild's, taken before Angular's final optimisation pass, so they add up to more than the total. Rank by them; budget by the total._",
);
console.log(out.join('\n'));

/**
 * The initial outputs are Angular's `main`, `polyfills` and `styles` entries
 * plus every chunk they reach by static import; dynamic imports and workers are
 * lazy. Bytes are raw output size, the same figure the budget checks.
 */
function analyse(meta) {
  const outputs = meta.outputs;
  // `entryPoint` rules out main's CSS sibling, which Angular never counts.
  const pending = Object.keys(outputs).filter(
    (file) => outputs[file].entryPoint && /^(main|polyfills|styles)-[^/]*\.(js|css)$/.test(file),
  );
  const initial = new Set();
  while (pending.length) {
    const file = pending.pop();
    if (initial.has(file) || !outputs[file]) continue;
    initial.add(file);
    for (const imp of outputs[file].imports ?? []) {
      if (imp.kind === 'import-statement') pending.push(imp.path);
    }
  }

  let total = 0;
  const modules = new Map();
  for (const file of initial) {
    total += outputs[file].bytes;
    for (const [input, { bytesInOutput }] of Object.entries(outputs[file].inputs)) {
      const mod = moduleOf(input);
      modules.set(mod, (modules.get(mod) ?? 0) + bytesInOutput);
    }
  }

  const importers = new Map();
  for (const [input, { imports }] of Object.entries(meta.inputs)) {
    for (const imp of imports ?? []) {
      if (imp.kind !== 'import-statement' && imp.kind !== 'require-call') continue;
      const [from, to] = [moduleOf(input), moduleOf(imp.path)];
      if (from === to) continue;
      if (!importers.has(to)) importers.set(to, new Set());
      importers.get(to).add(from);
    }
  }
  const importersOf = (mod) => {
    const all = [...(importers.get(mod) ?? [])]
      .filter((m) => modules.has(m))
      .map((m) => `\`${m}\``);
    return all.length > 3
      ? `${all.slice(0, 3).join(', ')} +${all.length - 3} more`
      : all.join(', ');
  };

  return { total, modules, importersOf };
}

/** npm files collapse to their package; everything else stays a source path. */
function moduleOf(input) {
  const at = input.lastIndexOf('node_modules/');
  if (at < 0) return input;
  const parts = input.slice(at + 'node_modules/'.length).split('/');
  return parts[0].startsWith('@') ? `${parts[0]}/${parts[1]}` : parts[0];
}
