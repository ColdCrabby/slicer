// Local worker entry point for Monaco's base editor worker. Importing the
// module for its side effects wires up `self.onmessage`. Referencing this file
// via `new Worker(new URL('./editor.worker', import.meta.url))` lets the
// bundler (esbuild) emit a real, base-href-relative worker asset — unlike a
// bare module specifier, which esbuild cannot resolve inside `new URL(...)`.
import 'monaco-editor/editor/editor.worker.js';
