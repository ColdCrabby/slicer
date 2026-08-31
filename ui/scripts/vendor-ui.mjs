// Fetches the shared Cold Crabby UI (ColdCrabby/ui) into vendor/coldcrabby-ui.
//
// The slicer consumes that repo as raw source — a tsconfig path resolves
// `@coldcrabby/ui` to its `public-api.ts`, and Sass `includePaths` pulls in its
// design language — so there is no published package to install. The checkout
// is git-ignored and tracks `main`. This script runs on `postinstall` (clone if
// missing) and can be run directly to update to the latest `main`.

import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const REPO = 'https://github.com/ColdCrabby/ui.git';
const BRANCH = 'main';

const here = dirname(fileURLToPath(import.meta.url));
const dest = join(here, '..', 'vendor', 'coldcrabby-ui');

const run = (args, cwd) =>
  execFileSync('git', args, { cwd, stdio: 'inherit' });

if (existsSync(join(dest, '.git'))) {
  // Already vendored — fast-forward to the tip of main.
  run(['fetch', '--depth', '1', 'origin', BRANCH], dest);
  run(['reset', '--hard', `origin/${BRANCH}`], dest);
} else {
  run(['clone', '--depth', '1', '--branch', BRANCH, REPO, dest]);
}

console.log('vendor-ui: coldcrabby-ui is up to date on', BRANCH);
