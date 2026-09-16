import { DOCUMENT, Injector, runInInjectionContext } from '@angular/core';
import { describe, expect, it } from 'vitest';
import { Viewport } from './viewport';

type Listener = (event: { pointerType: string; timeStamp: number }) => void;

/**
 * Stand-in document, so the pointer rules can be exercised without a DOM.
 *
 * Not a real one: the service only wants `matchMedia`, two event listeners and
 * a class list, and a jsdom `PointerEvent` is not guaranteed to exist. `coarse`
 * decides what every media query answers, which is enough — the queries
 * themselves are asserted by the constants, not by a browser.
 */
function fakeDocument(coarse: boolean) {
  const listeners: Listener[] = [];
  const classes = new Set<string>();
  const document = {
    documentElement: {
      classList: {
        toggle: (name: string, on: boolean) => (on ? classes.add(name) : classes.delete(name)),
      },
    },
    defaultView: {
      matchMedia: () => ({ matches: coarse, addEventListener: () => {} }),
      addEventListener: (_type: string, listener: Listener) => listeners.push(listener),
    },
  };
  return {
    document: document as unknown as Document,
    classes,
    /** Deliver one pointer event to every listener the service registered. */
    point: (pointerType: string, timeStamp: number) => {
      for (const listener of listeners) {
        listener({ pointerType, timeStamp });
      }
    },
  };
}

function make(coarse: boolean) {
  const fake = fakeDocument(coarse);
  const injector = Injector.create({
    providers: [{ provide: DOCUMENT, useValue: fake.document }],
  });
  return { ...fake, viewport: runInInjectionContext(injector, () => new Viewport()) };
}

describe('Viewport pointer precision', () => {
  it('assumes a fingertip until a pen proves otherwise', () => {
    const { viewport } = make(true);
    expect(viewport.isCoarsePointer()).toBe(true);
    expect(viewport.isStylus()).toBe(false);
    expect(viewport.isFingertip()).toBe(true);
  });

  it('is never a fingertip where there is a cursor', () => {
    const { viewport } = make(false);
    expect(viewport.isFingertip()).toBe(false);
  });

  it('hands the cursor sizes back while a pen is in use', () => {
    const { viewport, classes, point } = make(true);
    point('pen', 1000);
    expect(viewport.isStylus()).toBe(true);
    expect(viewport.isFingertip()).toBe(false);
    expect(classes.has('is-stylus')).toBe(true);
  });

  it('ignores the hand resting on the glass mid-stroke', () => {
    const { viewport, point } = make(true);
    point('pen', 1000);
    point('touch', 1400);
    expect(viewport.isStylus()).toBe(true);
  });

  it('goes back to a fingertip once the pen has been away long enough', () => {
    const { viewport, classes, point } = make(true);
    point('pen', 1000);
    point('touch', 3000);
    expect(viewport.isStylus()).toBe(false);
    expect(viewport.isFingertip()).toBe(true);
    expect(classes.has('is-stylus')).toBe(false);
  });

  it('gives a mouse no grace period — it is not a palm', () => {
    const { viewport, point } = make(true);
    point('pen', 1000);
    point('mouse', 1100);
    expect(viewport.isStylus()).toBe(false);
  });
});
