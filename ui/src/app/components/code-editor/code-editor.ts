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
    if (!window.MonacoEnvironment) {
      window.MonacoEnvironment = {
        getWorker(_moduleId: string, label: string): Worker {
          switch (label) {
            case 'json':
              return new Worker(new URL('./workers/json.worker', import.meta.url), {
                type: 'module',
              });
            case 'css':
            case 'scss':
            case 'less':
              return new Worker(new URL('./workers/css.worker', import.meta.url), {
                type: 'module',
              });
            case 'html':
            case 'handlebars':
            case 'razor':
              return new Worker(new URL('./workers/html.worker', import.meta.url), {
                type: 'module',
              });
            case 'typescript':
            case 'javascript':
              return new Worker(new URL('./workers/ts.worker', import.meta.url), {
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

    // Dynamic import keeps the large Monaco bundle out of the initial
    // chunk — it is only fetched when the panel is first opened.
    const monaco = await import('monaco-editor');

    // Guarantee Monaco's structural CSS is applied before the editor is
    // created, otherwise it renders as a bare, unstyled textarea (see
    // `ensureMonacoStyles`).
    await ensureMonacoStyles();

    registerGcodeLanguage(monaco);

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
