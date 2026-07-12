#!/usr/bin/env node
/**
 * Compiler wrapper for building C++ code targeting wasm32-unknown-unknown.
 *
 * cc-rs passes --target=wasm32-unknown-unknown, but the WASI SDK's clang++
 * (wasm32-wasip1-clang++) needs to run without an explicit --target so it uses
 * its built-in default (wasm32-unknown-wasip1), which has the correct sysroot
 * and C++ multilib headers configured.
 *
 * Clipper2 is pure computation with no OS calls, so the resulting WASM object
 * files are compatible with wasm32-unknown-unknown.
 *
 * Usage:
 * - Windows: Set CXX_wasm32_unknown_unknown to tools/wasm32-clang++.cmd
 * - Linux/macOS: Set CXX_wasm32_unknown_unknown to tools/wasm32-clang++.js
 */

"use strict";

const { spawnSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

/**
 * Locate wasm32-wasip1-clang++ using three strategies, in priority order:
 *
 * 1. Already on PATH (user added WASI SDK bin/ to their shell PATH).
 * 2. WASI_SDK_PATH environment variable pointing at the SDK root.
 * 3. Common install locations (glob over versioned directory names).
 */
function findWasiClangpp() {
  const exe =
    process.platform === "win32"
      ? "wasm32-wasip1-clang++.exe"
      : "wasm32-wasip1-clang++";

  // 1. PATH lookup
  const pathDirs = (process.env.PATH || "").split(path.delimiter);
  for (const dir of pathDirs) {
    const candidate = path.join(dir, exe);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  // 2. WASI_SDK_PATH environment variable
  if (process.env.WASI_SDK_PATH) {
    const candidate = path.join(process.env.WASI_SDK_PATH, "bin", exe);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  // 3. Common install locations (versioned directory names like wasi-sdk-33.0-*)
  const searchRoots =
    process.platform === "win32"
      ? [os.homedir(), "C:\\", "C:\\Program Files"]
      : [os.homedir(), "/opt", "/usr/local"];

  for (const root of searchRoots) {
    if (!fs.existsSync(root)) {
      continue;
    }
    let entries;
    try {
      entries = fs.readdirSync(root);
    } catch {
      continue;
    }
    // Sort descending so the newest SDK version is preferred.
    const sdkDirs = entries
      .filter((e) => /^wasi-sdk/i.test(e))
      .sort()
      .reverse();
    for (const dir of sdkDirs) {
      const candidate = path.join(root, dir, "bin", exe);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }

  return null;
}

const WASI_CLANGPP = findWasiClangpp();
if (!WASI_CLANGPP) {
  console.error(
    "error: wasm32-wasip1-clang++ not found.\n" +
      "Install WASI SDK from https://github.com/WebAssembly/wasi-sdk/releases\n" +
      "then either:\n" +
      "  • Add <wasi-sdk>/bin to your PATH, or\n" +
      "  • Set WASI_SDK_PATH=<wasi-sdk root>",
  );
  process.exit(1);
}

// Filter out --target=* arguments added by cc-rs for the cargo target triple.
// The WASI SDK compiler uses its own built-in target (wasm32-unknown-wasip1)
// which has the correct sysroot and C++ multilib headers.
const args = process.argv
  .slice(2)
  .filter((a) => !a.startsWith("--target=") && a !== "-target");

// Disable C++ exceptions and RTTI so the compiled object files do not
// reference exception-ABI symbols (__cxa_throw, __cxa_allocate_exception,
// _ZTVN10__cxxabiv1…, _ZTI* typeinfos, etc.) that are unavailable when
// linking as wasm32-unknown-unknown without a C++ runtime.
// Clipper2 is pure computation; if it ever calls a path that would throw
// (e.g. std::vector capacity overflow), the WASI SDK translates this to
// std::terminate/abort — acceptable behaviour inside a WASM sandbox.
args.push("-fno-exceptions", "-fno-rtti");

const result = spawnSync(WASI_CLANGPP, args, { stdio: "inherit" });
process.exit(result.status ?? 1);
