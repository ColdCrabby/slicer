import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { FILAMENT_MATERIALS, FilamentMaterial, makeFilament } from '../../models/filament.model';
import { FilamentsStore } from '../../services/profiles/filaments-store';
import { Icon } from '../../shared/icon/icon';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { IconButton } from '../../ui/icon-button/icon-button';
import { SectionHeader } from '../../ui/section-header/section-header';

@Component({
  selector: 'nexus-settings-filaments',
  imports: [SectionHeader, EmptyState, Button, IconButton, Icon],
  templateUrl: './filaments.html',
  styleUrl: './filaments.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FilamentsSettings {
  protected readonly store = inject(FilamentsStore);
  protected readonly materials = FILAMENT_MATERIALS;

  add(): void {
    this.store.add(makeFilament());
  }

  remove(id: string): void {
    this.store.remove(id);
  }

  rename(id: string, event: Event): void {
    const name = (event.target as HTMLInputElement).value.trim();
    if (name) {
      this.store.update(id, { name });
    }
  }

  setColor(id: string, event: Event): void {
    this.store.update(id, { color: (event.target as HTMLInputElement).value });
  }

  setMaterial(id: string, event: Event): void {
    this.store.update(id, {
      material: (event.target as HTMLSelectElement).value as FilamentMaterial,
    });
  }
}
