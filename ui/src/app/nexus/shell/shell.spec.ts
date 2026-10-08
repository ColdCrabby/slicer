import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { RouterOutlet, provideRouter } from '@angular/router';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { OmniboxService } from '../../omnibox/omnibox-service';
import { Viewport } from '../../services/viewport';
import { AppShell } from './shell';

/**
 * The palette placeholder is enough to prove the mount/unmount contract: the
 * real `OmniboxPalette` pulls in the whole omnibox graph, which this template
 * test does not need and cannot usefully assert against. What matters is
 * whether the shell puts *something* with the palette's selector on screen while
 * `open` is true and takes it away the moment it turns false.
 */
@Component({
  selector: 'nexus-omnibox',
  template: '<p class="stub-palette">palette</p>',
})
class StubOmnibox {}

@Component({ selector: 'nexus-titlebar', template: '' })
class StubTitlebar {}

@Component({ selector: 'nexus-nav-rail', template: '' })
class StubNavRail {}

@Component({ selector: 'nexus-route-progress', template: '' })
class StubRouteProgress {}

describe('AppShell omnibox mount', () => {
  let fixture: ReturnType<typeof TestBed.createComponent<AppShell>>;
  let omnibox: OmniboxService;

  beforeEach(async () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [provideRouter([]), Viewport],
    });

    // The palette itself is behind `@defer`, and the real children are eager
    // chunks this test has no interest in; swap them for stubs so the deferred
    // block still has a component to resolve and mount.
    TestBed.overrideComponent(AppShell, {
      set: {
        imports: [RouterOutlet, StubTitlebar, StubNavRail, StubRouteProgress, StubOmnibox],
      },
    });
    await TestBed.compileComponents();

    omnibox = TestBed.inject(OmniboxService);
    fixture = TestBed.createComponent(AppShell);
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  });

  afterEach(() => {
    TestBed.resetTestingModule();
  });

  const paletteOnScreen = (): boolean =>
    fixture.nativeElement.querySelector('.stub-palette') !== null;

  it('keeps the palette out of the DOM until the service opens it', () => {
    expect(paletteOnScreen()).toBe(false);
  });

  it('removes the palette again when the service hides it', async () => {
    omnibox.show();
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(paletteOnScreen()).toBe(true);

    omnibox.hide();
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();

    // The regression this guards: `@defer (when omnibox.open())` is a one-way
    // latch, so without an inner `@if` the palette opened once and never left —
    // Escape set `open` to false and nothing happened on screen.
    expect(paletteOnScreen()).toBe(false);
  });
});
