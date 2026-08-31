import { provideHttpClient } from '@angular/common/http';
import {
  type ApplicationConfig,
  Injector,
  inject,
  provideAppInitializer,
  provideBrowserGlobalErrorListeners,
} from '@angular/core';
import { provideRouter, TitleStrategy, withPreloading } from '@angular/router';
import { provideMarkdown } from 'ngx-markdown';
import { environment } from '../environments/environment';
import { APP_ROUTES } from './app-routes';
import { AccentService } from './services/accent';
import { CATALOG_SOURCE } from './services/catalog/cloud-catalog';
import { provideCatalogClient } from './services/catalog/catalog-client';
import { RemoteCatalogSource } from './services/catalog/remote-catalog-source';
import { KeyboardShortcuts } from './services/keyboard-shortcuts/keyboard-shortcuts';
import { NavigationProgress } from './services/navigation-progress';
import { provideProfilePersistence } from './services/profiles/profile-persistence';
import { ProfileSync } from './services/profiles/profile-sync';
import { IdleRoutePreload } from './services/route-preload';
import { NexusTitleStrategy } from './services/title-strategy';
import { UploadGuard } from './services/upload-guard';
import { UserInputModality } from '@coldcrabby/ui';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(APP_ROUTES, withPreloading(IdleRoutePreload)),
    { provide: TitleStrategy, useClass: NexusTitleStrategy },
    {
      provide: CATALOG_SOURCE,
      useFactory: () => new RemoteCatalogSource(environment.catalogApiUrl, inject(Injector)),
    },
    provideHttpClient(),
    provideCatalogClient(environment.catalogApiUrl),
    provideMarkdown(),
    provideProfilePersistence(),
    provideAppInitializer(() => {
      inject(AccentService);
      inject(KeyboardShortcuts);
      inject(UserInputModality);
      inject(UploadGuard);
      inject(ProfileSync);
      // Has to exist before the first navigation starts, or the app's very
      // first (and slowest, uncached) route transition is the one it misses.
      inject(NavigationProgress);
    }),
  ],
};
