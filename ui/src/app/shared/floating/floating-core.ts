import {
  arrow,
  autoUpdate,
  computePosition,
  flip,
  hide,
  offset,
  shift,
  size,
} from '@floating-ui/dom';
import type { Middleware, Placement, ReferenceElement, Strategy } from '@floating-ui/dom';

/** Placement of the floating element relative to its reference. */
export type FloatingPlacement = Placement;

/**
 * Anything Floating UI can anchor to: a real DOM element or a virtual element
 * exposing a `getBoundingClientRect()` (used for cursor-following popovers).
 */
export type FloatingReference = ReferenceElement;

export interface FloatingOptions {
  /** Preferred side. Floating UI flips/shifts away from this to stay on-screen. */
  placement?: FloatingPlacement;
  /** `fixed` (default) escapes clipping/`overflow` ancestors; `absolute` for in-flow hosts. */
  strategy?: Strategy;
  /** Gap between reference and floating element, in px. Default `6`. */
  offset?: number;
  /** Flip to the opposite side when the preferred side doesn't fit. Default `true`. */
  flip?: boolean;
  /** Slide along the placement axis to stay in view. Default `true`. */
  shift?: boolean;
  /** Padding kept from the viewport edge for flip/shift/size, in px. Default `8`. */
  padding?: number;
  /** Constrain the floating element to the available space (sets `max-width`/`max-height`). */
  size?: boolean;
  /** Arrow element to position against the reference, if any. */
  arrowEl?: HTMLElement | null;
  /** Match the floating element's width to the reference width. */
  matchReferenceWidth?: boolean;
  /** Toggle `visibility` when the reference is fully clipped/escaped. Default `true`. */
  hideWhenDetached?: boolean;
}

/** The arrow is a rotated square of this side length (px). */
const ARROW_SIZE = 8;

/**
 * Positions `floating` relative to `reference` using Floating UI, keeping it
 * on-screen via flip + shift and (optionally) fitting it to the available
 * space. Returns a cleanup function that stops the auto-update loop.
 *
 * This is the framework-agnostic heart of the Nexus floating system; Angular
 * plumbing (portals, change detection, dismissal) lives in `FloatingService`.
 */
export function applyFloating(
  reference: FloatingReference,
  floating: HTMLElement,
  options: FloatingOptions = {},
): () => void {
  const {
    placement = 'top',
    strategy = 'fixed',
    offset: mainOffset = 6,
    flip: enableFlip = true,
    shift: enableShift = true,
    padding = 8,
    size: enableSize = false,
    arrowEl = null,
    matchReferenceWidth = false,
    hideWhenDetached = true,
  } = options;

  floating.style.position = strategy;
  floating.style.top = '0';
  floating.style.left = '0';

  const update = (): void => {
    const middleware: Middleware[] = [offset(mainOffset + (arrowEl ? ARROW_SIZE / 2 : 0))];

    if (enableFlip) {
      middleware.push(flip({ padding }));
    }
    if (enableShift) {
      middleware.push(shift({ padding }));
    }
    if (enableSize) {
      middleware.push(
        size({
          padding,
          apply({ availableWidth, availableHeight, elements }) {
            Object.assign(elements.floating.style, {
              maxWidth: `${Math.max(0, availableWidth)}px`,
              maxHeight: `${Math.max(0, availableHeight)}px`,
            });
          },
        }),
      );
    }
    if (arrowEl) {
      middleware.push(arrow({ element: arrowEl, padding: 4 }));
    }
    if (hideWhenDetached) {
      middleware.push(hide());
    }

    if (matchReferenceWidth && reference instanceof HTMLElement) {
      floating.style.width = `${reference.getBoundingClientRect().width}px`;
    }

    void computePosition(reference, floating, { placement, strategy, middleware }).then(
      ({ x, y, placement: finalPlacement, middlewareData }) => {
        Object.assign(floating.style, {
          transform: `translate(${Math.round(x)}px, ${Math.round(y)}px)`,
        });
        floating.dataset['placement'] = finalPlacement;

        if (hideWhenDetached) {
          const hidden = middlewareData.hide?.referenceHidden ?? false;
          floating.style.visibility = hidden ? 'hidden' : 'visible';
        }

        if (arrowEl && middlewareData.arrow) {
          const { x: ax, y: ay } = middlewareData.arrow;
          const side = finalPlacement.split('-')[0] as 'top' | 'right' | 'bottom' | 'left';
          const opposite = { top: 'bottom', right: 'left', bottom: 'top', left: 'right' } as const;
          Object.assign(arrowEl.style, {
            left: ax != null ? `${ax}px` : '',
            top: ay != null ? `${ay}px` : '',
            right: '',
            bottom: '',
            [opposite[side]]: `${-ARROW_SIZE / 2}px`,
          });
        }
      },
    );
  };

  return autoUpdate(reference, floating, update, {
    animationFrame: !(reference instanceof HTMLElement),
  });
}
