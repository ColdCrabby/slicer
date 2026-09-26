import { DOCUMENT, Injector, runInInjectionContext } from '@angular/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ExternalLinks } from './external-links';

const openUrl = vi.fn();
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl }));

const globals = globalThis as { __TAURI_INTERNALS__?: unknown };

/** Stand-in document with one click handler slot and an app origin. */
function fakeDocument(origin = 'tauri://localhost') {
  let handler: ((event: unknown) => void) | null = null;
  return {
    document: {
      location: { origin },
      addEventListener: (_type: string, h: (event: unknown) => void) => (handler = h),
    } as unknown as Document,
    click: (href: string, init: { button?: number; defaultPrevented?: boolean } = {}) => {
      const url = new URL(href, origin === 'tauri://localhost' ? 'http://x.invalid' : origin);
      const anchor = { href: url.href, origin: url.origin };
      const event = {
        button: init.button ?? 0,
        defaultPrevented: init.defaultPrevented ?? false,
        target: { closest: (sel: string) => (sel === 'a[href]' ? anchor : null) },
        preventDefault: vi.fn(),
      };
      handler?.(event);
      return event;
    },
    get listening() {
      return handler !== null;
    },
  };
}

function create(doc: Document): void {
  const injector = Injector.create({ providers: [{ provide: DOCUMENT, useValue: doc }] });
  runInInjectionContext(injector, () => new ExternalLinks());
}

describe('ExternalLinks', () => {
  beforeEach(() => {
    globals.__TAURI_INTERNALS__ = {};
    openUrl.mockClear();
  });
  afterEach(() => {
    delete globals.__TAURI_INTERNALS__;
  });

  it('hands an http(s) link to the system browser in a native shell', async () => {
    const fake = fakeDocument();
    create(fake.document);
    const event = fake.click('https://github.com/ColdCrabby/cloud-presets');
    expect(event.preventDefault).toHaveBeenCalled();
    await vi.waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith('https://github.com/ColdCrabby/cloud-presets'),
    );
  });

  it("leaves the app's own routes in the webview under tauri dev", () => {
    const fake = fakeDocument('http://localhost:4213');
    create(fake.document);
    const event = fake.click('http://localhost:4213/settings');
    expect(event.preventDefault).toHaveBeenCalledTimes(0);
  });

  it('ignores clicks something else already handled, and non-primary buttons', () => {
    const fake = fakeDocument();
    create(fake.document);
    expect(
      fake.click('https://a.test', { defaultPrevented: true }).preventDefault,
    ).toHaveBeenCalledTimes(0);
    expect(fake.click('https://a.test', { button: 1 }).preventDefault).toHaveBeenCalledTimes(0);
  });

  it('does nothing in a browser', () => {
    delete globals.__TAURI_INTERNALS__;
    const fake = fakeDocument();
    create(fake.document);
    expect(fake.listening).toBe(false);
  });
});
