import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  effect,
  inject,
  input,
  output,
  viewChild,
} from '@angular/core';
import type * as Monaco from 'monaco-editor';
import { NEXUS_CODE_THEME, registerGcodeLanguage } from './gcode-language';

// Extend the window type to allow the MonacoEnvironment global required by the
// Monaco editor loader.
declare global {
  interface Window {
    MonacoEnvironment?: Monaco.Environment;
  }
}

/**
 * Monaco's structural stylesheet — the concatenated `editor.main.css` from the
 * `min` distribution, copied to `/assets/monaco` by the build (see
 * `angular.json`). It carries the layout rules (`.view-lines`,
 * `.inputarea { position: absolute }`, …) and inlines the codicon font as a
 * base64 data URI, so it is fully self-contained.
 */
const MONACO_CSS_ASSET = 'assets/monaco/min/vs/editor/editor.main.css';

/**
 * Shared promise that resolves once Monaco's structural CSS has been applied.
 *
 * Angular's esbuild output splits the CSS that Monaco's ESM modules import into
 * a lazy chunk that is *not* reliably attached to the page when the
 * dynamically-imported `monaco-editor` JS resolves. When `editor.create()` wins
 * that race the editor renders with **no layout rules** — the hidden input
 * textarea falls back to `position: static` and appears as a bare, resizable
 * `<textarea>` (the token colours still show because Monaco injects those at
 * runtime via JS). This was intermittent locally but reproduced reliably on the
 * slower GitHub Pages / WASM deploy.
 *
 * Loading the self-contained `editor.main.css` ourselves and awaiting it before
 * `editor.create()` removes the race entirely. The href is resolved against the
 * document base so it works under a sub-path base href too.
 */
let monacoStylesReady: Promise<void> | null = null;

function ensureMonacoStyles(): Promise<void> {
  if (monacoStylesReady) {
    return monacoStylesReady;
  }

  monacoStylesReady = new Promise<void>((resolve) => {
    const href = new URL(MONACO_CSS_ASSET, document.baseURI).href;

    const existing = document.querySelector<HTMLLinkElement>(
      `link[rel="stylesheet"][href="${href}"]`,
    );
    if (existing) {
      resolve();
      return;
    }

    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = href;
    // Never block editor creation forever: resolve on load *or* error. A missing
    // stylesheet only degrades styling, and the lazy chunk may still supply it.
    link.addEventListener('load', () => resolve(), { once: true });
    link.addEventListener('error', () => resolve(), { once: true });
    document.head.appendChild(link);
  });

  return monacoStylesReady;
}

/** The subset of Monaco this app touches — the editor and language registries. */
type MonacoApi = Pick<typeof Monaco, 'editor' | 'languages'>;

/**
 * Languages that cost a network fetch to support, and how to pull each one in.
 *
 * Every editor in the app is one of two things: a G-code field (printer
 * start/end scripts, the schema form's `gcode-field`) or a read-only JSON view
 * (the operation-pipeline dialog). Nothing else can appear, because `language`
 * is only ever bound to a literal in a template.
 *
 * G-code is absent here because it is *ours* — a Monarch grammar registered
 * with the shared theme on every load (see {@link loadMonaco}). JSON is
 * Monaco's own language service and brings a web worker with it, so it is
 * fetched only when a JSON editor is actually mounted.
 */
const MONACO_LANGUAGES = new Map<string, () => Promise<unknown>>([
  ['json', () => import('monaco-editor/language/json/monaco.contribution')],
]);

/** Resolves once the editor core is loaded and the shared theme is defined. */
let monacoReady: Promise<MonacoApi> | null = null;

/**
 * In-flight or settled load per language.
 *
 * Keyed on the *promise*, not on a "seen" flag: the operation-pipeline dialog
 * mounts two JSON editors at once, and a flag set before the import resolves
 * would let the second one call `editor.create` while the language is still
 * arriving — leaving it stuck on plaintext.
 */
const loadedLanguages = new Map<string, Promise<unknown>>();

/**
 * Fetch the Monaco editor, plus whatever `language` needs beyond it.
 *
 * **Never import the `monaco-editor` package root.** That barrel is
 * `editor.main`, which registers all ~90 bundled grammars *and* the TypeScript,
 * CSS and HTML language services — a 2.7 MB chunk plus ~9 MB of web workers, for
 * an app that shows G-code and JSON. Composing the same editor from the
 * package's modular entry points instead costs a fraction of that, and drops the
 * unused workers from the deployed output entirely:
 *
 * - `editor/editor.api` — the editor itself, with no language attached.
 * - `features/register.all` — the standard editor contributions (find, folding,
 *   bracket matching, context menu, …). The editor is unusably bare without
 *   them, and they are what `foldingStrategy` relies on.
 *
 * Everything is memoised, so this is a no-op for every editor after the first.
 */
async function loadMonaco(language: string): Promise<MonacoApi> {
  monacoReady ??= (async () => {
    const [monaco] = await Promise.all([
      import('monaco-editor/editor/editor.api'),
      import('monaco-editor/features/register.all'),
    ]);
    // Defines NEXUS_CODE_THEME as well as the grammar, so this has to run for
    // JSON editors too — `editor.create` throws on an unknown theme id.
    registerGcodeLanguage(monaco);
    return monaco;
  })();

  const monaco = await monacoReady;

  const load = MONACO_LANGUAGES.get(language);
  if (load) {
    let pending = loadedLanguages.get(language);
    if (!pending) {
      pending = load();
      loadedLanguages.set(language, pending);
    }
    await pending;
  }

  return monaco;
}

/**
 * Thin Angular wrapper around the Monaco editor.
 *
 * The component initialises the editor exactly once, after the host element
 * has been inserted into the DOM. Destroying the component disposes the
 * editor instance so its WebGL / DOM resources are released.
 *
 * The editor is intentionally bare-bones: language, initial value and other
 * options can be extended via `@Input()` when needed.
 */
@Component({
  selector: 'nexus-code-editor',
  standalone: true,
  template: `<div class="editor-mount" #mount></div>`,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        overflow: hidden;
        background: var(--color-surface, #1e1e1e);
      }

      .editor-mount {
        flex: 1;
        min-height: 0;
      }
    `,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CodeEditor {
  /** Text content to display. Changing this signal updates the editor live. */
  readonly content = input('');
  /** Monaco language identifier (default: `'plaintext'`). */
  readonly language = input('plaintext');
  /** When true the editor is read-only. */
  readonly readOnly = input(false);
  /** Emits the editor's text whenever the user edits it. */
  readonly contentChange = output<string>();

  private readonly mount = viewChild.required<ElementRef<HTMLDivElement>>('mount');
  private editor: Monaco.editor.IStandaloneCodeEditor | null = null;
  /** Guards the change output from firing during programmatic `setValue`. */
  private applyingExternal = false;

  constructor() {
    const destroyRef = inject(DestroyRef);

    afterNextRender(async () => {
      await this.initMonaco();
    });

    // Push content / readOnly changes into the live editor whenever they change.
    effect(() => {
      const value = this.content();
      const readOnly = this.readOnly();
      if (this.editor) {
        if (this.editor.getValue() !== value) {
          this.applyingExternal = true;
          this.editor.setValue(value);
          this.applyingExternal = false;
        }
        this.editor.updateOptions({ readOnly });
      }
    });

    destroyRef.onDestroy(() => {
      this.editor?.dispose();
      this.editor = null;
    });
  }

  private async initMonaco(): Promise<void> {
    // Tell Monaco where to find its web workers. Each worker is referenced via
    // `new Worker(new URL(..., import.meta.url))` so the bundler (esbuild)
    // emits a real, content-hashed worker asset and rewrites the URL relative
    // to the deployed base href. The previous Blob + `importScripts('bare
    // specifier')` approach produced URLs that only resolved at the site root,
    // so on a static/GitHub Pages deploy served from a sub-path the workers
    // 404'd and Monaco failed to initialise.
    //
    // Only the two workers this app can actually ask for are listed. Naming a
    // worker here is what makes the bundler emit it, and the TypeScript one
    // alone is a 7 MB asset — see `MONACO_LANGUAGES` for why no other language
    // can reach this switch.
    if (!window.MonacoEnvironment) {
      window.MonacoEnvironment = {
        getWorker(_moduleId: string, label: string): Worker {
          switch (label) {
            case 'json':
              return new Worker(new URL('./workers/json.worker', import.meta.url), {
                type: 'module',
              });
            default:
              return new Worker(new URL('./workers/editor.worker', import.meta.url), {
                type: 'module',
              });
          }
        },
      };
    }

    const monaco = await loadMonaco(this.language());

    // Guarantee Monaco's structural CSS is applied before the editor is
    // created, otherwise it renders as a bare, unstyled textarea (see
    // `ensureMonacoStyles`).
    await ensureMonacoStyles();

    this.editor = monaco.editor.create(this.mount().nativeElement, {
      value: this.content(),
      language: this.language(),
      theme: NEXUS_CODE_THEME,
      automaticLayout: true,
      fontSize: 13,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      wordWrap: 'on',
      lineNumbers: 'on',
      readOnly: this.readOnly(),
      folding: true,
      foldingStrategy: 'indentation',
      showFoldingControls: 'always',
    });

    this.editor.onDidChangeModelContent(() => {
      if (!this.applyingExternal && this.editor) {
        this.contentChange.emit(this.editor.getValue());
      }
    });
  }
}
