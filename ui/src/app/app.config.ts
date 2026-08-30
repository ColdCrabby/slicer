import { provideHttpClient } from '@angular/common/http';
import {
  type ApplicationConfig,
  inject,
  provideAppInitializer,
  provideBrowserGlobalErrorListeners,
} from '@angular/core';
import { provideRouter, TitleStrategy } from '@angular/router';
import { provideMarkdown } from 'ngx-markdown';
import { environment } from '../environments/environment';
import { APP_ROUTES } from './app-routes';
import { AccentService } from './services/accent';
import { CATALOG_SOURCE } from './services/catalog/cloud-catalog';
import { configureCatalogClient } from './services/catalog/catalog-client';
import { RemoteCatalogSource } from './services/catalog/remote-catalog-source';
import { KeyboardShortcuts } from './services/keyboard-shortcuts/keyboard-shortcuts';
import { provideProfilePersistence } from './services/profiles/profile-persistence';
import { ProfileSync } from './services/profiles/profile-sync';
import { NexusTitleStrategy } from './services/title-strategy';
import { UploadGuard } from './services/upload-guard';
import { UserInputModality } from '@coldcrabby/ui';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(APP_ROUTES),
    { provide: TitleStrategy, useClass: NexusTitleStrategy },
    {
      provide: CATALOG_SOURCE,
      useFactory: () => new RemoteCatalogSource(environment.catalogApiUrl),
    },
    provideHttpClient(),
    provideMarkdown(),
    provideProfilePersistence(),
    provideAppInitializer(() => {
      configureCatalogClient(environment.catalogApiUrl);
      inject(AccentService);
      inject(KeyboardShortcuts);
      inject(UserInputModality);
      inject(UploadGuard);
      inject(ProfileSync);
    }),
  ],
};
