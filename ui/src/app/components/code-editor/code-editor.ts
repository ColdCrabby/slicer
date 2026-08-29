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
