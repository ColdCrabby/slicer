import { provideHttpClient } from '@angular/common/http';
import {
  ApplicationConfig,
  inject,
  provideAppInitializer,
  provideBrowserGlobalErrorListeners,
} from '@angular/core';
import { provideRouter } from '@angular/router';
import { provideMarkdown } from 'ngx-markdown';
import { APP_ROUTES } from './app-routes';
import { AccentService } from './services/accent';
import { KeyboardShortcuts } from './services/keyboard-shortcuts/keyboard-shortcuts';
import { UploadGuard } from './services/upload-guard';
import { UserInputModality } from './shared/input-modality/input-modality';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(APP_ROUTES),
    provideHttpClient(),
    provideMarkdown(),
    provideAppInitializer(() => {
      inject(AccentService);
      inject(KeyboardShortcuts);
      inject(UserInputModality);
      inject(UploadGuard);
    }),
  ],
};
