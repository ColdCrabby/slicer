#!/usr/bin/env node
//
// build-wasm.mjs — build the wasm library and run wasm-bindgen over it.
//
// The only reason this is a script rather than two shell commands: the C++
// compiler wrapper for Clipper2 comes in two spellings, and which one cargo
// may spawn depends on the *host*, not the target.
//
//     Windows        tools/wasm32-clang++.cmd   (a batch file)
//     macOS / Linux  tools/wasm32-clang++.js    (a shebang script)
//
// `.cargo/config.toml` can only name one of them, and cargo's `[env]` has no
// way to branch on the host — so it names the Windows one and this script
// overrides it for everyone else. A real environment variable wins over an
// `[env]` entry, which is exactly the escape hatch that override needs.
//
// Usage:
//   node scripts/build-wasm.mjs                  # scene engine only
//   node scripts/build-wasm.mjs --web-slicer     # + the browser slicer
//
// Without --web-slicer nothing pulls in Clipper2's C++ at all, so the wrapper
// never runs and the WASI SDK is not needed.

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const webSlicer = process.argv.includes('--web-slicer');

const wrapper = path.join(
  repoRoot,
  'tools',
  process.platform === 'win32' ? 'wasm32-clang++.cmd' : 'wasm32-clang++.js',
);

const target = 'wasm32-unknown-unknown';
const steps = [
  {
    command: 'cargo',
    args: [
      'build',
      '--lib',
      '--target',
      target,
      '--release',
      ...(webSlicer ? ['--features', 'web-slicer'] : []),
    ],
  },
  {
    command: 'wasm-bindgen',
    args: [
      `target/${target}/release/slicer_engine.wasm`,
      '--target',
      'web',
      '--out-dir',
      'ui/src/generated/scene-wasm',
      '--out-name',
      'scene_engine',
    ],
  },
];

for (const { command, args } of steps) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: 'inherit',
    shell: process.platform === 'win32',
    env: { ...process.env, CXX_wasm32_unknown_unknown: wrapper },
  });

  if (result.error) {
    console.error(`build-wasm: could not run ${command}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}
