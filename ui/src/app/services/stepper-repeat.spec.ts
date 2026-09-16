import { DOCUMENT, Injector, runInInjectionContext } from '@angular/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { REPEAT_BUTTON_SELECTOR, StepperRepeat } from './stepper-repeat';

type Handler = (event: unknown) => void;

/**
 * Stand-in document, so the repeat can be driven without a DOM.
 *
 * The service is a set of document-level listeners and a timer, which is all
 * this has to offer: somewhere to register a handler and a way to fire one.
 */
function fakeDocument() {
  const listeners = new Map<string, Handler[]>();
  const add = (type: string, handler: Handler): void => {
    listeners.set(type, [...(listeners.get(type) ?? []), handler]);
  };
  return {
    document: {
      addEventListener: add,
      defaultView: { addEventListener: add },
    } as unknown as Document,
    fire: (type: string, event: unknown): void => {
      for (const handler of listeners.get(type) ?? []) {
        handler(event);
      }
    },
  };
}

/** A stepper button, or something that only looks like one. */
function fakeButton({ isStepper = true, disabled = false } = {}) {
  const clicks: unknown[] = [];
  const button = {
    disabled,
    isConnected: true,
    // Answers only the real selector, so a typo in it fails the test rather
    // than quietly matching everything.
    closest: (selector: string) =>
      isStepper && selector === REPEAT_BUTTON_SELECTOR ? button : null,
    dispatchEvent: (event: unknown) => {
      clicks.push(event);
      return true;
    },
  };
  return { button, clicks };
}

const press = (target: unknown) => ({ button: 0, target, shiftKey: false, altKey: false });

describe('StepperRepeat', () => {
  let fake: ReturnType<typeof fakeDocument>;

  beforeEach(() => {
    vi.useFakeTimers();
    fake = fakeDocument();
    const injector = Injector.create({
      providers: [{ provide: DOCUMENT, useValue: fake.document }],
    });
    runInInjectionContext(injector, () => new StepperRepeat());
  });

  afterEach(() => vi.useRealTimers());

  it('keeps stepping while a stepper is held', () => {
    const { button, clicks } = fakeButton();
    fake.fire('pointerdown', press(button));
    vi.advanceTimersByTime(2000);
    expect(clicks.length).toBeGreaterThan(1);
  });

  it('accelerates the longer the press is held', () => {
    const { button, clicks } = fakeButton();
    fake.fire('pointerdown', press(button));
    vi.advanceTimersByTime(1000);
    const early = clicks.length;
    vi.advanceTimersByTime(1000);
    expect(clicks.length - early).toBeGreaterThan(early);
  });

  it('leaves a press that is not on a stepper alone', () => {
    const { button, clicks } = fakeButton({ isStepper: false });
    fake.fire('pointerdown', press(button));
    vi.advanceTimersByTime(2000);
    expect(clicks).toHaveLength(0);
  });

  it('leaves a disabled stepper alone', () => {
    const { button, clicks } = fakeButton({ disabled: true });
    fake.fire('pointerdown', press(button));
    vi.advanceTimersByTime(2000);
    expect(clicks).toHaveLength(0);
  });

  it('stops once the stepper disables itself at its limit', () => {
    const { button, clicks } = fakeButton();
    fake.fire('pointerdown', press(button));
    vi.advanceTimersByTime(1000);
    const atLimit = clicks.length;
    button.disabled = true;
    vi.advanceTimersByTime(2000);
    expect(clicks).toHaveLength(atLimit);
  });

  it('stops when the press ends', () => {
    const { button, clicks } = fakeButton();
    fake.fire('pointerdown', press(button));
    vi.advanceTimersByTime(1000);
    const onLift = clicks.length;
    fake.fire('pointerup', {});
    vi.advanceTimersByTime(2000);
    expect(clicks).toHaveLength(onLift);
  });

  // The touch regression: a held press is the gesture, and Android Chrome reads
  // it as a request for a context menu about half a second in — which is barely
  // after the repeat starts.
  it('survives the context menu a touchscreen raises over the stepper', () => {
    const { button, clicks } = fakeButton();
    fake.fire('pointerdown', press(button));
    vi.advanceTimersByTime(600);
    const beforeMenu = clicks.length;
    const preventDefault = vi.fn();
    fake.fire('contextmenu', { target: button, preventDefault });
    vi.advanceTimersByTime(1000);
    expect(preventDefault).toHaveBeenCalled();
    expect(clicks.length).toBeGreaterThan(beforeMenu);
  });

  it('still stops for a context menu opened anywhere else', () => {
    const { button, clicks } = fakeButton();
    const elsewhere = fakeButton({ isStepper: false }).button;
    fake.fire('pointerdown', press(button));
    vi.advanceTimersByTime(1000);
    const onMenu = clicks.length;
    fake.fire('contextmenu', { target: elsewhere, preventDefault: vi.fn() });
    vi.advanceTimersByTime(2000);
    expect(clicks).toHaveLength(onMenu);
  });

  it('ignores anything but the primary button', () => {
    const { button, clicks } = fakeButton();
    fake.fire('pointerdown', { ...press(button), button: 2 });
    vi.advanceTimersByTime(2000);
    expect(clicks).toHaveLength(0);
  });
});
