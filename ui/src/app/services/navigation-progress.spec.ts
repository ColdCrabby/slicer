import { TestBed } from '@angular/core/testing';
import { signal } from '@angular/core';
import { NavigationEnd, NavigationError, NavigationStart, Router } from '@angular/router';
import { Subject } from 'rxjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AppVersion } from './app-version';
import { NavigationProgress } from './navigation-progress';

/**
 * `NavigationProgress` exists to make lazily-loaded routes honest without being
 * noisy, so what is pinned here is exactly that trade-off: silence for fast
 * navigations, a visible wait for slow ones, and a reload prompt when a chunk
 * is gone for good.
 */
function setup() {
  const events = new Subject<unknown>();
  const staleReports: number[] = [];

  TestBed.configureTestingModule({
    providers: [
      { provide: Router, useValue: { events } },
      {
        provide: AppVersion,
        useValue: {
          updateAvailable: signal(false),
          reportStaleAssets: () => staleReports.push(1),
        },
      },
    ],
  });

  return { progress: TestBed.inject(NavigationProgress), events, staleReports };
}

describe('NavigationProgress', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('stays silent for a navigation that resolves quickly', () => {
    const { progress, events } = setup();

    events.next(new NavigationStart(1, '/settings'));
    vi.advanceTimersByTime(50);
    events.next(new NavigationEnd(1, '/settings', '/settings/general'));
    vi.advanceTimersByTime(1000);

    expect(progress.active()).toBe(false);
  });

  it('shows the wait once a navigation outlasts the threshold', () => {
    const { progress, events } = setup();

    events.next(new NavigationStart(1, '/settings'));
    expect(progress.active()).toBe(false);

    vi.advanceTimersByTime(200);
    expect(progress.active()).toBe(true);
    expect(progress.visiblePendingUrl()).toBe('/settings');
  });

  it('animates out rather than cutting away when a shown navigation lands', () => {
    const { progress, events } = setup();

    events.next(new NavigationStart(1, '/settings'));
    vi.advanceTimersByTime(200);
    events.next(new NavigationEnd(1, '/settings', '/settings/general'));

    expect(progress.active()).toBe(true);
    expect(progress.complete()).toBe(true);

    vi.advanceTimersByTime(500);
    expect(progress.active()).toBe(false);
  });

  it('marks only the destination being loaded, not every ancestor of it', () => {
    const { progress, events } = setup();

    events.next(new NavigationStart(1, '/settings/printers'));
    vi.advanceTimersByTime(200);

    expect(progress.isPendingUnder('/settings')).toBe(true);
    expect(progress.isPendingUnder('/settings/printers')).toBe(true);
    // Every URL is nominally "under" the root; Home must not light up on the
    // way to somewhere else.
    expect(progress.isPendingUnder('/')).toBe(false);
    expect(progress.isPendingUnder('/slice')).toBe(false);
  });

  it('does not treat a sibling with a shared prefix as the destination', () => {
    const { progress, events } = setup();

    events.next(new NavigationStart(1, '/settings/printers'));
    vi.advanceTimersByTime(200);

    expect(progress.isPendingUnder('/settings/printer')).toBe(false);
  });

  it('ignores query strings when matching the destination', () => {
    const { progress, events } = setup();

    events.next(new NavigationStart(1, '/slice/new?from=home'));
    vi.advanceTimersByTime(200);

    expect(progress.isPendingUnder('/slice')).toBe(true);
  });

  it('prompts for a reload when a route chunk can no longer be fetched', () => {
    const { events, staleReports } = setup();

    events.next(new NavigationStart(1, '/settings'));
    events.next(
      new NavigationError(
        1,
        '/settings',
        new TypeError('Failed to fetch dynamically imported module: /chunk-ABC.js'),
      ),
    );

    expect(staleReports).toHaveLength(1);
  });

  it('leaves other navigation failures alone — a reload would not fix them', () => {
    const { events, staleReports } = setup();

    events.next(new NavigationStart(1, '/settings'));
    events.next(new NavigationError(1, '/settings', new Error('Guard rejected')));

    expect(staleReports).toHaveLength(0);
  });

  it('clears the wait when a navigation fails', () => {
    const { progress, events } = setup();

    events.next(new NavigationStart(1, '/settings'));
    vi.advanceTimersByTime(200);
    events.next(new NavigationError(1, '/settings', new Error('Guard rejected')));
    vi.advanceTimersByTime(500);

    expect(progress.active()).toBe(false);
  });
});
