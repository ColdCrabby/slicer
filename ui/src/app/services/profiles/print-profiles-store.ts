import { Injectable } from '@angular/core';
import { DEFAULT_PRINT_PROFILES } from '../../models/print-profile.model';
import type { PrintProfile } from '../../models/print-profile.model';
import { LocalCollectionStore } from './local-collection-store';

@Injectable({ providedIn: 'root' })
export class PrintProfilesStore extends LocalCollectionStore<PrintProfile> {
  constructor() {
    // The UI's "print profiles" are the engine's `processes` category.
    super('profiles.printProfiles', DEFAULT_PRINT_PROFILES, 'processes');
  }
}
