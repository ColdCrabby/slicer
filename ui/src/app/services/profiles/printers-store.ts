import { Injectable } from '@angular/core';
import { DEFAULT_PRINTERS } from '../../models/printer.model';
import type { PrinterProfile } from '../../models/printer.model';
import { LocalCollectionStore } from './local-collection-store';

@Injectable({ providedIn: 'root' })
export class PrintersStore extends LocalCollectionStore<PrinterProfile> {
  constructor() {
    super('profiles.printers.v2', DEFAULT_PRINTERS, 'printers');
  }
}
