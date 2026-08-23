import { Injectable } from '@angular/core';
import { DEFAULT_PRINT_PROFILES, PrintProfile } from '../../models/print-profile.model';
import { LocalCollectionStore } from './local-collection-store';

@Injectable({ providedIn: 'root' })
export class PrintProfilesStore extends LocalCollectionStore<PrintProfile> {
  constructor() {
    super('profiles.printProfiles', DEFAULT_PRINT_PROFILES);
  }
}
