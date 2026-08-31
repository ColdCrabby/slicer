import { bootstrapApplication } from '@angular/platform-browser';
import { appConfig } from './app/app.config';
import { App } from './app/app';

/**
 * Clears the boot splash painted by `index.html`.
 *
 * Declared there rather than here, because the splash has to be able to end
 * itself even if this bundle never arrives — see the script at the bottom of
 * the document.
 */
declare global {
  interface Window {
    __nexusSplashDone?: () => void;
  }
}

/**
 * Dismiss the splash once the app is genuinely on screen.
 *
 * Called on failure too: a progress bar frozen over a blank page tells the user
 * nothing, while letting the splash go at least reveals whatever state the app
 * did reach.
 */
function dismissSplash(): void {
  window.__nexusSplashDone?.();
}

bootstrapApplication(App, appConfig)
  .then(dismissSplash)
  .catch((err) => {
    dismissSplash();
    console.error(err);
  });
