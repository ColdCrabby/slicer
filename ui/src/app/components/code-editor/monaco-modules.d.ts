/**
 * Monaco ships its modular entry points as plain JavaScript with no type
 * declarations of their own — only the package root (`monaco-editor`) and
 * `editor/editor.api` carry `.d.ts` files.
 *
 * The language contributions are imported purely for their side effect (they
 * register a language with the already-typed `monaco.languages` registry), so
 * an opaque module declaration is the whole contract. See `code-editor.ts` for
 * why the modular entry points are used instead of the package root.
 */
declare module 'monaco-editor/features/register.all';
declare module 'monaco-editor/language/json/monaco.contribution';
