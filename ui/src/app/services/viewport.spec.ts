import { DOCUMENT, Injector, runInInjectionContext } from '@angular/core';
import { describe, expect, it } from 'vitest';
import { Viewport } from './viewport';

type Listener = (event: { pointerType: string; timeStamp: number }) => void;

/**
 * Stand-in document, so the pointer rules can be exercised without a DOM.
 *
 * Not a real one: the service only wants `matchMedia`, event listeners and a
 * class list, and a jsdom `PointerEvent` is not guaranteed to exist. `coarse`
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
  it('marks a coarse pointer for the touch sizes', () => {
    const { viewport, classes } = make(true);
    expect(viewport.isCoarsePointer()).toBe(true);
    expect(classes.has('is-coarse-pointer')).toBe(true);
  });

  it('is never coarse where there is a cursor', () => {
    const { viewport, classes } = make(false);
    expect(viewport.isCoarsePointer()).toBe(false);
    expect(classes.has('is-coarse-pointer')).toBe(false);
  });

  // Resizing on every pen/finger swap made the whole interface flicker.
  it('keeps one size however the pointer in hand changes', () => {
    const { viewport, classes, point } = make(true);
    const before = [...classes].sort();
    for (const [type, at] of [
      ['pen', 1000],
      ['touch', 1100],
      ['pen', 1200],
      ['touch', 4000],
    ] as const) {
      point(type, at);
      expect(viewport.isCoarsePointer()).toBe(true);
      expect([...classes].sort()).toEqual(before);
    }
  });
});
