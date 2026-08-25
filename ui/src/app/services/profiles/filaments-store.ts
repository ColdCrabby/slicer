import { Injectable } from '@angular/core';
import { DEFAULT_FILAMENTS } from '../../models/filament.model';
import type { FilamentProfile } from '../../models/filament.model';
import { LocalCollectionStore } from './local-collection-store';

@Injectable({ providedIn: 'root' })
export class FilamentsStore extends LocalCollectionStore<FilamentProfile> {
  constructor() {
    super('profiles.filaments', DEFAULT_FILAMENTS, 'filaments');
  }
}
