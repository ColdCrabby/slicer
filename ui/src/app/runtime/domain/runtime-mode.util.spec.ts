import { afterEach, describe, expect, it } from 'vitest';
import { isTauriDesktop, isTauriHost, isTauriMobile } from './runtime-mode.util';

/**
 * These helpers decide whether a `@tauri-apps/api` module gated behind
 * `#[cfg(desktop)]` may be called. Getting iPadOS wrong is not a cosmetic bug:
 * the whole `menu` module is absent there, so a misclassified iPad silently
 * loses its context menus entirely.
 */

type Globals = {
  __TAURI_INTERNALS__?: unknown;
  navigator?: unknown;
};

const globals = globalThis as Globals;
const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, 'navigator');

function setHost(present: boolean): void {
  if (present) {
    globals.__TAURI_INTERNALS__ = {};
  } else {
    delete globals.__TAURI_INTERNALS__;
  }
}

function setNavigator(userAgent: string, platform: string, maxTouchPoints: number): void {
  Object.defineProperty(globalThis, 'navigator', {
    value: { userAgent, platform, maxTouchPoints },
    configurable: true,
  });
}

// A plain Mac. Note this UA must not contain "Tauri": `isTauriHost` also sniffs
// the user-agent string, so including it would make the browser cases pass for
// the wrong reason.
const MAC = [
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Safari/605.1.15',
  'MacIntel',
  0,
] as const;
// iPadOS deliberately masquerades as a Mac: same "like Mac OS X" UA string and
// a "MacIntel" platform. Touch points are the only reliable discriminator.
const IPAD_DESKTOP_UA = [
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15',
  'MacIntel',
  5,
] as const;
const IPAD_MOBILE_UA = [
  'Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15',
  'iPad',
  5,
] as const;

afterEach(() => {
  setHost(false);
  if (originalNavigator) {
    Object.defineProperty(globalThis, 'navigator', originalNavigator);
  } else {
    delete globals.navigator;
  }
});

describe('isTauriHost', () => {
  it('is false in a plain browser', () => {
    setNavigator(...MAC);
    expect(isTauriHost()).toBe(false);
  });

  it('is true when Tauri internals are present', () => {
    setHost(true);
    setNavigator(...MAC);
    expect(isTauriHost()).toBe(true);
  });
});

describe('isTauriMobile', () => {
  it('is false outside a Tauri host, even on an iPad user agent', () => {
    setNavigator(...IPAD_MOBILE_UA);
    expect(isTauriMobile()).toBe(false);
  });

  it('detects an iPad that reports an iPad user agent', () => {
    setHost(true);
    setNavigator(...IPAD_MOBILE_UA);
    expect(isTauriMobile()).toBe(true);
  });

  it('detects an iPad masquerading as a Mac, via touch points', () => {
    setHost(true);
    setNavigator(...IPAD_DESKTOP_UA);
    expect(isTauriMobile()).toBe(true);
  });

  it('does not misclassify a real Mac as mobile', () => {
    setHost(true);
    setNavigator(...MAC);
    expect(isTauriMobile()).toBe(false);
  });
});

describe('isTauriDesktop', () => {
  it('is true only for a Tauri host with desktop chrome', () => {
    setHost(true);
    setNavigator(...MAC);
    expect(isTauriDesktop()).toBe(true);
  });

  it('is false on iPad, so callers fall back instead of hitting a missing API', () => {
    setHost(true);
    setNavigator(...IPAD_MOBILE_UA);
    expect(isTauriDesktop()).toBe(false);

    setNavigator(...IPAD_DESKTOP_UA);
    expect(isTauriDesktop()).toBe(false);
  });

  it('is false in a browser', () => {
    setNavigator(...MAC);
    expect(isTauriDesktop()).toBe(false);
  });
});
