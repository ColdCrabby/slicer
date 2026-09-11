import { describe, expect, it } from 'vitest';
import { apiDocsUrlFor } from './titlebar';

/**
 * Only the cloud runtime talks to an HTTP server. The desktop app drives the
 * engine over Tauri commands and the web build runs it in a worker, so in both
 * of those the reference has no host and the link must not appear at all —
 * a topbar button that leads nowhere is worse than a missing one.
 */
describe('API reference link', () => {
  it('points at this slicer, not a hosted copy', () => {
    expect(apiDocsUrlFor('cloud', 'https://printer.local/api')).toBe(
      'https://printer.local/api/docs',
    );
  });

  it('is absent where there is no server behind the app', () => {
    expect(apiDocsUrlFor('native', 'https://printer.local/api')).toBeNull();
    expect(apiDocsUrlFor('web', 'https://printer.local/api')).toBeNull();
  });
});
