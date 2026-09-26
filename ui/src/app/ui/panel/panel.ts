import { Directive, booleanAttribute, input } from '@angular/core';

/**
 * Makes its host one of the page's panels: a rounded, bordered box floating on
 * the window's background, `--panel-gap` from its neighbours.
 *
 * A page is laid out as panels rather than as one surface ruled into regions —
 * the Settings section list, a profile list, its editor, the 3D scene and the
 * drawer over it are each their own box. The directive only names the role;
 * the look is `styles/components/_panels.scss`, emitted once, and the tokens
 * are in `theme/_shell.scss`. The page decides where panels sit and how big
 * they are.
 *
 * `primary` marks the page's primary content — the scene, the editor, the page
 * of preferences — and is set on exactly one panel per page: it alone takes
 * the darker surface, and every other panel keeps the window's tone.
 *
 * `raised` is for a panel laid *over* another that has to lift off busy
 * content — a flyout. It gains a shadow.
 *
 * ```html
 * <aside nexusPanel>…</aside>
 * <main nexusPanel primary>…</main>
 * <div nexusPanel raised>…</div>
 * ```
 */
@Directive({
  selector: '[nexusPanel]',
  standalone: true,
  host: {
    class: 'nexus-panel',
    '[class.is-primary]': 'primary()',
    '[class.is-raised]': 'raised()',
  },
})
export class Panel {
  /** The page's primary content — one per page. */
  readonly primary = input(false, { transform: booleanAttribute });
  /** Laid over another panel, lifted off it with a shadow. */
  readonly raised = input(false, { transform: booleanAttribute });
}
