import { describe, expect, it } from 'vitest';
import { OmniboxService } from './omnibox-service';

/**
 * The service is the palette's door, and several hands hold keys to it: the
 * keyboard table, the shell's `@defer` trigger, the settings hand-offs — and,
 * now, the titlebar's search button for hands without a keyboard. The contract
 * they all lean on: `open` is the truth, a prefill is taken exactly once, and
 * a bare open never inherits the last one's query.
 */
describe('OmniboxService', () => {
  it('starts closed', () => {
    expect(new OmniboxService().open()).toBe(false);
  });

  it('toggles between open and closed', () => {
    const service = new OmniboxService();
    service.toggle();
    expect(service.open()).toBe(true);
    service.toggle();
    expect(service.open()).toBe(false);
  });

  it('hands a prefill to the first opener and to no one after', () => {
    const service = new OmniboxService();
    service.show('settings retraction');
    expect(service.takePrefill()).toBe('settings retraction');
    expect(service.takePrefill()).toBeNull();
  });

  it('never carries a prefill into a bare open', () => {
    const service = new OmniboxService();
    service.show('settings retraction');
    service.hide();
    service.show();
    expect(service.takePrefill()).toBeNull();
  });
});
