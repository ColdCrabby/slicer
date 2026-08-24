import type * as Monaco from 'monaco-editor';

/** Monaco language id for G-code. */
export const GCODE_LANGUAGE_ID = 'gcode';

/** Shared theme id used by every {@link CodeEditor}. Inherits `vs-dark`. */
export const NEXUS_CODE_THEME = 'nexus-code';

let registered = false;

/**
 * Register the G-code language + the shared editor theme with Monaco.
 *
 * Idempotent — safe to call for every editor instance; the actual work runs
 * once. The theme inherits `vs-dark`, so JSON/plaintext editors are unaffected;
 * it only adds colour rules for the G-code-specific token types below.
 */
export function registerGcodeLanguage(monaco: typeof Monaco): void {
  if (registered) {
    return;
  }
  registered = true;

  monaco.languages.register({ id: GCODE_LANGUAGE_ID });

  monaco.languages.setMonarchTokensProvider(GCODE_LANGUAGE_ID, {
    ignoreCase: true,
    tokenizer: {
      root: [
        // Line comments (`; ...`) and parenthesised comments (`( ... )`).
        [/;.*$/, 'comment'],
        [/\([^)]*\)/, 'comment'],
        // Slice-time placeholders such as `{nozzle_temp}`.
        [/\{[^}]*\}/, 'variable.placeholder'],
        // G / M / T commands (G1, M104, G0.1, T0).
        [/\b[gmt]\d+(?:\.\d+)?\b/, 'keyword'],
        // Axis / parameter words (X, Y, Z, E, F, S, P, …).
        [/\b[a-z](?=[-+]?[\d.{])/, 'attribute.name'],
        // Numeric operands.
        [/[-+]?\d*\.?\d+/, 'number'],
      ],
    },
  });

  monaco.editor.defineTheme(NEXUS_CODE_THEME, {
    base: 'vs-dark',
    inherit: true,
    rules: [
      { token: 'comment', foreground: '6a9955', fontStyle: 'italic' },
      { token: 'keyword', foreground: '4fc1ff' },
      { token: 'attribute.name', foreground: 'dcdcaa' },
      { token: 'number', foreground: 'b5cea8' },
      { token: 'variable.placeholder', foreground: 'ffa657', fontStyle: 'bold' },
    ],
    colors: {},
  });
}
