import type { Routes } from '@angular/router';
import { NexusSlicingShell } from './nexus/layout/slicing-shell/slicing-shell';
import { AppShell } from './nexus/shell/shell';
import { SettingsShell } from './pages/settings/settings-shell';
import { uploadCanDeactivate } from './services/upload-guard';

export const APP_ROUTES: Routes = [
  {
    path: '',
    component: AppShell,
    children: [
      {
        path: '',
        loadComponent: async () => import('./pages/home/home').then((m) => m.HomeDashboard),
      },
      {
        path: 'slice',
        component: NexusSlicingShell,
        children: [
          { path: '', redirectTo: 'new', pathMatch: 'full' },
          {
            path: 'new',
            loadComponent: () => import('./pages/slice-new/slice-new').then((m) => m.SliceNew),
            canDeactivate: [uploadCanDeactivate],
          },
          {
            path: ':requestUuid',
            loadComponent: () =>
              import('./pages/slice-viewer/slice-viewer').then((m) => m.SliceViewer),
          },
        ],
      },
      {
        path: 'settings',
        component: SettingsShell,
        children: [
          { path: '', redirectTo: 'general', pathMatch: 'full' },
          {
            path: 'general',
            loadComponent: () => import('./pages/settings/general').then((m) => m.GeneralSettings),
          },
          {
            path: 'appearance',
            loadComponent: () =>
              import('./pages/settings/appearance').then((m) => m.AppearanceSettings),
          },
          {
            path: 'printers',
            loadComponent: () =>
              import('./pages/settings/printers').then((m) => m.PrintersSettings),
          },
          {
            path: 'filaments',
            loadComponent: () =>
              import('./pages/settings/filaments').then((m) => m.FilamentsSettings),
          },
          {
            path: 'profiles',
            loadComponent: () =>
              import('./pages/settings/profiles').then((m) => m.ProfilesSettings),
          },
          {
            path: 'shortcuts',
            loadComponent: () =>
              import('./pages/settings/shortcuts').then((m) => m.ShortcutsSettings),
          },
        ],
      },
      {
        path: 'components',
        loadComponent: () =>
          import('./pages/ui-components/ui-components.component').then((m) => m.UiComponentsPage),
      },
    ],
  },
];
