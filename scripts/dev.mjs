#!/usr/bin/env node
//
// dev.mjs — start the dev stack on a seeded, conflict-free set of ports.
//
// One host often runs several checkouts at once (a worktree per branch, an
// agent session, a colleague over SSH). Fixed ports make those instances fight
// over :4213/:5201 and, worse, over the engine's work directory. A *seed* gives
// each instance its own lane:
//
//     UI   http://localhost:4<seed>      Angular dev server
//     API  http://127.0.0.1:5<seed>      engine (serve)
//     work $TMPDIR/slicer-engine-dev-<seed>
//
// The seed is a three-digit number (200-999); pick one at random and every port
// follows from it. The browser never needs the API port: the dev server proxies
// /api and /ws to it (see ui/proxy.conf.mjs), so the UI URL is the only thing
// worth reporting.
//
// Usage:
//   pnpm run dev                     # random seed, engine + UI
//   pnpm run dev -- --seed 742       # exact seed (fails if it is taken)
//   pnpm run dev -- --ui-only        # UI alone
//   pnpm run dev -- --backend-only   # engine alone
//   pnpm run dev:web-slicer          # wasm slicer, no engine
//   pnpm run dev:desktop             # Tauri shell + seeded UI dev server
//   pnpm run dev -- --print          # resolve ports, print JSON, start nothing
//
// SLICER_DEV_SEED is honoured as the default for --seed.

import { spawn } from 'node:child_process';
import net from 'node:net';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const SEED_MIN = 200;
const SEED_MAX = 999;
const UI_BASE = 4000;
const API_BASE = 5000;

// ---------------------------------------------------------------- arguments

function parseArgs(argv) {
  const opts = {
    seed: process.env.SLICER_DEV_SEED ?? null,
    seedExplicit: process.env.SLICER_DEV_SEED != null,
    backend: true,
    ui: true,
    webSlicer: false,
    desktop: false,
    print: false,
  };

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    switch (arg) {
      case '--seed':
        opts.seed = argv[++i];
        opts.seedExplicit = true;
        break;
      case '--ui-only':
      case '--no-backend':
        opts.backend = false;
        break;
      case '--backend-only':
        opts.ui = false;
        break;
      case '--web-slicer':
        opts.webSlicer = true;
        opts.backend = false;
        break;
      case '--desktop':
        opts.desktop = true;
        opts.backend = false;
        break;
      case '--print':
        opts.print = true;
        break;
      case '-h':
      case '--help':
        opts.help = true;
        break;
      default:
        if (arg.startsWith('--seed=')) {
          opts.seed = arg.slice('--seed='.length);
          opts.seedExplicit = true;
        } else {
          die(`unknown argument: ${arg}\nRun with --help for usage.`);
        }
    }
  }

  if (opts.seed != null) {
    const parsed = Number(opts.seed);
    if (!Number.isInteger(parsed) || parsed < SEED_MIN || parsed > SEED_MAX) {
      die(`--seed must be an integer between ${SEED_MIN} and ${SEED_MAX} (got "${opts.seed}")`);
    }
    opts.seed = parsed;
  }

  return opts;
}

function die(message) {
  console.error(`dev: ${message}`);
  process.exit(1);
}

const HELP = `Start the dev stack on a seeded, conflict-free set of ports.

  pnpm run dev [-- <options>]

Options:
  --seed <200-999>   Use this seed instead of a random one (fails if taken).
  --ui-only          Angular dev server only.
  --backend-only     Engine (serve) only.
  --web-slicer       Wasm browser slicer; implies --ui-only.
  --desktop          Tauri desktop shell against a seeded UI dev server.
  --print            Print the resolved ports as JSON and exit.

Ports follow the seed: UI on 4<seed>, engine on 5<seed>.`;

// -------------------------------------------------------------------- ports

function portFree(port) {
  const tryHost = (host) =>
    new Promise((resolve) => {
      const server = net.createServer();
      server.once('error', () => resolve(false));
      server.once('listening', () => server.close(() => resolve(true)));
      server.listen(port, host);
    });
  // Check both: a process bound to one of them still blocks the other in
  // practice, and different tools bind differently.
  return Promise.all([tryHost('0.0.0.0'), tryHost('127.0.0.1')]).then((r) => r.every(Boolean));
}

const portsOf = (seed) => ({ seed, ui: UI_BASE + seed, api: API_BASE + seed });

async function seedIsFree(seed, opts) {
  const { ui, api } = portsOf(seed);
  const needed = [];
  if (opts.ui || opts.desktop) needed.push(ui);
  if (opts.backend) needed.push(api);
  for (const port of needed) {
    if (!(await portFree(port))) return false;
  }
  return true;
}

async function resolveSeed(opts) {
  if (opts.seedExplicit) {
    if (!(await seedIsFree(opts.seed, opts))) {
      const { ui, api } = portsOf(opts.seed);
      die(
        `seed ${opts.seed} is already in use (UI ${ui} / engine ${api}).\n` +
          `Another instance is probably running — drop --seed to roll a free one.`,
      );
    }
    return opts.seed;
  }

  for (let attempt = 0; attempt < 100; attempt++) {
    const seed = SEED_MIN + Math.floor(Math.random() * (SEED_MAX - SEED_MIN + 1));
    if (await seedIsFree(seed, opts)) return seed;
  }
  die('could not find a free seed after 100 attempts — is something binding every port?');
}

// ------------------------------------------------------------------ helpers

// `cargo run -- serve` refuses to start without a UI directory, but in dev the
// Angular dev server is the UI. Hand the engine a stub so the check passes and
// a stray hit on the API port explains where the real UI lives.
function ensurePlaceholderUiDir(uiPort) {
  const dir = path.join(repoRoot, 'target', 'dev-ui');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(
    path.join(dir, 'index.html'),
    `<!doctype html>
<meta charset="utf-8">
<title>Cold Crabby — engine only</title>
<p>This is the engine's API/WebSocket port. The dev UI is at
<a href="http://localhost:${uiPort}/">http://localhost:${uiPort}/</a>.</p>
`,
  );
  return dir;
}

const COLORS = { api: '\u001b[36m', ui: '\u001b[35m', app: '\u001b[33m', reset: '\u001b[0m' };

const children = [];
let shuttingDown = false;

// Windows cannot spawn `pnpm`/`cargo` (shim .cmd files) without a shell, and a
// shell does not quote arguments for us — so quote anything with a space.
const onWindows = process.platform === 'win32';
const quote = (arg) => (onWindows && /\s/.test(arg) ? `"${arg}"` : arg);

function start(name, command, args, env = {}) {
  const child = spawn(command, onWindows ? args.map(quote) : args, {
    cwd: repoRoot,
    env: { ...process.env, ...env },
    stdio: ['inherit', 'pipe', 'pipe'],
    shell: onWindows,
  });
  children.push({ name, child });

  const prefix = `${COLORS[name] ?? ''}[${name}]${COLORS.reset} `;
  const pipe = (stream, sink) => {
    let buffered = '';
    stream.setEncoding('utf8');
    stream.on('data', (chunk) => {
      buffered += chunk;
      const lines = buffered.split('\n');
      buffered = lines.pop() ?? '';
      for (const line of lines) sink.write(`${prefix}${line}\n`);
    });
    stream.on('end', () => {
      if (buffered) sink.write(`${prefix}${buffered}\n`);
    });
  };
  pipe(child.stdout, process.stdout);
  pipe(child.stderr, process.stderr);

  child.on('exit', (code, signal) => {
    if (shuttingDown) return;
    console.error(`\n${prefix}exited (${signal ?? `code ${code}`}) — stopping the rest.`);
    shutdown(code ?? 1);
  });
  child.on('error', (error) => {
    console.error(`${prefix}failed to start: ${error.message}`);
    shutdown(1);
  });

  return child;
}

function shutdown(code) {
  if (shuttingDown) return;
  shuttingDown = true;
  // Set it up front: once the children are gone the event loop drains and the
  // process exits on its own, before the SIGKILL sweep below ever fires.
  process.exitCode = code;
  for (const { child } of children) {
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
  }
  // Give them a moment to go down cleanly, then leave.
  setTimeout(() => {
    for (const { child } of children) {
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
    }
    process.exit(code);
  }, 2000).unref();
}

// --------------------------------------------------------------------- main

const opts = parseArgs(process.argv.slice(2));
if (opts.help) {
  console.log(HELP);
  process.exit(0);
}

const seed = await resolveSeed(opts);
const { ui: uiPort, api: apiPort } = portsOf(seed);
const workDir = path.join(os.tmpdir(), `slicer-engine-dev-${seed}`);

if (opts.print) {
  console.log(
    JSON.stringify(
      { seed, uiPort, apiPort, uiUrl: `http://localhost:${uiPort}/`, workDir },
      null,
      2,
    ),
  );
  process.exit(0);
}

const lines = [`Seed ${seed}`];
if (opts.ui && !opts.desktop) lines.push(`  UI      http://localhost:${uiPort}/   <- open this`);
if (opts.desktop) lines.push(`  UI      http://localhost:${uiPort}/   (Tauri window)`);
if (opts.backend) {
  lines.push(`  Engine  http://127.0.0.1:${apiPort}/  (proxied at /api and /ws)`);
  lines.push(`  Work    ${workDir}`);
}
console.log(`${lines.join('\n')}\n`);

if (opts.backend) {
  fs.mkdirSync(workDir, { recursive: true });
  start(
    'api',
    'cargo',
    [
      'run',
      '--',
      'serve',
      '--port',
      String(apiPort),
      '--ui-dir',
      ensurePlaceholderUiDir(uiPort),
      '--work-dir',
      workDir,
    ],
    { SLICER_DEV_SEED: String(seed) },
  );
}

if (opts.ui || opts.desktop) {
  const script = opts.webSlicer ? 'start:web-slicer' : 'start';
  start('ui', 'pnpm', ['--filter', 'slicer-ui', 'run', script, '--port', String(uiPort)], {
    SLICER_API_PORT: String(apiPort),
    SLICER_DEV_SEED: String(seed),
  });
}

if (opts.desktop) {
  // The Tauri config pins devUrl to the default port and starts a UI dev server
  // of its own; override both so the shell attaches to the seeded one above.
  // Written to a file rather than passed inline — the JSON's quotes do not
  // survive a Windows shell.
  const overrides = path.join(repoRoot, 'target', `tauri.dev.${seed}.json`);
  fs.mkdirSync(path.dirname(overrides), { recursive: true });
  fs.writeFileSync(
    overrides,
    JSON.stringify({ build: { devUrl: `http://localhost:${uiPort}`, beforeDevCommand: '' } }),
  );
  start(
    'app',
    'pnpm',
    ['--filter', 'slicer-ui-desktop', 'exec', 'tauri', 'dev', '--config', overrides],
    { SLICER_DEV_SEED: String(seed) },
  );
}

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => {
    console.log('\nStopping…');
    shutdown(0);
  });
}
