import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { makePrintProfile, PRINT_QUALITIES, PrintQuality } from '../../models/print-profile.model';
import { PrintProfilesStore } from '../../services/profiles/print-profiles-store';
import { Icon } from '../../shared/icon/icon';
import { Button } from '../../ui/button/button';
import { EmptyState } from '../../ui/empty-state/empty-state';
import { IconButton } from '../../ui/icon-button/icon-button';
import { SectionHeader } from '../../ui/section-header/section-header';

@Component({
  selector: 'nexus-settings-profiles',
  imports: [SectionHeader, EmptyState, Button, IconButton, Icon],
  templateUrl: './profiles.html',
  styleUrl: './profiles.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ProfilesSettings {
  protected readonly store = inject(PrintProfilesStore);
  protected readonly qualities = PRINT_QUALITIES;

  add(): void {
    this.store.add(makePrintProfile());
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

  setQuality(id: string, event: Event): void {
    this.store.update(id, { quality: (event.target as HTMLSelectElement).value as PrintQuality });
  }

  infillPct(fraction: number): number {
    return Math.round(fraction * 100);
  }
}
