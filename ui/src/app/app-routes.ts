import { inject } from '@angular/core';
import { Router, type Routes } from '@angular/router';
import { NexusSlicingShell } from './nexus/layout/slicing-shell/slicing-shell';
import { AppShell } from './nexus/shell/shell';
import { SettingsShell } from './pages/settings/settings-shell';
import { SlicerFile } from './services/slicer-file';
import { uploadCanDeactivate } from './services/upload-guard';

export const APP_ROUTES: Routes = [
  {
    path: '',
    component: AppShell,
    children: [
      {
        path: '',
        title: 'Home',
        loadComponent: async () => import('./pages/home/home').then((m) => m.HomeDashboard),
      },
      {
        path: 'slice',
        component: NexusSlicingShell,
        children: [
          {
            // Land on the active workplate if one is open; otherwise start a
            // new plate. A static `redirectTo: 'new'` would drop the user back
            // to the empty "Start your first plate" screen even while a
            // workplate is still loaded.
            path: '',
            pathMatch: 'full',
            redirectTo: () => {
              const uuid = inject(SlicerFile).requestUuid();
              return inject(Router).createUrlTree(['/slice', uuid ?? 'new']);
            },
          },
          {
            path: 'new',
            title: 'New Slice',
            loadComponent: () => import('./pages/slice-new/slice-new').then((m) => m.SliceNew),
            canDeactivate: [uploadCanDeactivate],
          },
          {
            path: ':requestUuid',
            title: 'Slice Preview',
            loadComponent: () =>
              import('./pages/slice-viewer/slice-viewer').then((m) => m.SliceViewer),
          },
        ],
      },
      {
        path: 'settings',
        component: SettingsShell,
        title: 'Settings',
        children: [
          { path: '', redirectTo: 'general', pathMatch: 'full' },
          {
            path: 'general',
            title: 'General Settings',
            loadComponent: () => import('./pages/settings/general').then((m) => m.GeneralSettings),
          },
          {
            path: 'appearance',
            title: 'Appearance Settings',
            loadComponent: () =>
              import('./pages/settings/appearance').then((m) => m.AppearanceSettings),
          },
          {
            path: 'printers',
            title: 'Printer Settings',
            loadComponent: () =>
              import('./pages/settings/printers').then((m) => m.PrintersSettings),
          },
          {
            path: 'printers/new',
            title: 'Add Printer',
            loadComponent: () =>
              import('./components/profiles/printer-wizard').then((m) => m.PrinterWizard),
          },
          {
            path: 'filaments',
            title: 'Filament Settings',
            loadComponent: () =>
              import('./pages/settings/filaments').then((m) => m.FilamentsSettings),
          },
          {
            path: 'filaments/new',
            title: 'Add Filament',
            loadComponent: () =>
              import('./components/profiles/filament-wizard').then((m) => m.FilamentWizard),
          },
          {
            path: 'profiles',
            title: 'Profile Settings',
            loadComponent: () =>
              import('./pages/settings/profiles').then((m) => m.ProfilesSettings),
          },
          {
            path: 'profiles/new',
            title: 'Add Print Profile',
            loadComponent: () =>
              import('./components/profiles/profile-wizard').then((m) => m.ProfileWizard),
          },
          {
            path: 'labels',
            title: 'Label Settings',
            loadComponent: () => import('./pages/settings/labels').then((m) => m.LabelsSettings),
          },
          {
            path: 'shortcuts',
            title: 'Keyboard Shortcuts',
            loadComponent: () =>
              import('./pages/settings/shortcuts').then((m) => m.ShortcutsSettings),
          },
          {
            path: 'changelog',
            title: "What's New",
            loadComponent: () =>
              import('./pages/settings/changelog').then((m) => m.ChangelogSettings),
          },
          {
            path: 'danger-zone',
            title: 'Danger Zone',
            loadComponent: () =>
              import('./pages/settings/danger-zone').then((m) => m.DangerZoneSettings),
          },
        ],
      },
      {
        path: 'components',
        title: 'UI Components',
        loadComponent: () =>
          import('./pages/ui-components/ui-components.component').then((m) => m.UiComponentsPage),
      },
    ],
  },
];
