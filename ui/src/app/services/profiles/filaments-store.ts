import { Injectable } from '@angular/core';
import { DEFAULT_FILAMENTS, FilamentProfile } from '../../models/filament.model';
import { LocalCollectionStore } from './local-collection-store';

@Injectable({ providedIn: 'root' })
export class FilamentsStore extends LocalCollectionStore<FilamentProfile> {
  constructor() {
    super('profiles.filaments.v2', DEFAULT_FILAMENTS);
  }
}
