import { provideHttpClient } from '@angular/common/http';
import {
  type ApplicationConfig,
  inject,
  provideAppInitializer,
  provideBrowserGlobalErrorListeners,
} from '@angular/core';
import { provideRouter, TitleStrategy } from '@angular/router';
import { provideMarkdown } from 'ngx-markdown';
import { APP_ROUTES } from './app-routes';
import { AccentService } from './services/accent';
import { KeyboardShortcuts } from './services/keyboard-shortcuts/keyboard-shortcuts';
import { provideProfilePersistence } from './services/profiles/profile-persistence';
import { ProfileSync } from './services/profiles/profile-sync';
import { NexusTitleStrategy } from './services/title-strategy';
import { UploadGuard } from './services/upload-guard';
import { UserInputModality } from './shared/input-modality/input-modality';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(APP_ROUTES),
    { provide: TitleStrategy, useClass: NexusTitleStrategy },
    provideHttpClient(),
    provideMarkdown(),
    provideProfilePersistence(),
    provideAppInitializer(() => {
      inject(AccentService);
      inject(KeyboardShortcuts);
      inject(UserInputModality);
      inject(UploadGuard);
      inject(ProfileSync);
    }),
  ],
};
