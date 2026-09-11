import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { OpenWorkplateTab, OpenWorkplates } from '../../services/open-workplates';
import { Slicer } from '../../services/slicer';
import { WorkplateNames } from '../../services/workplate-names';
import { Icon, IconButton, TooltipDirective } from '@coldcrabby/ui';

/**
 * Open-workplate tab strip shown in the titlebar, replacing the single
 * editable plate-name field. Each tab is an independently renamed, switchable
 * workplate (see {@link OpenWorkplates}); the `+` opens a fresh one.
 */
@Component({
  selector: 'nexus-workplate-tabs',
  imports: [Icon, IconButton, TooltipDirective],
  templateUrl: './workplate-tabs.html',
  styleUrl: './workplate-tabs.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    class: 'nexus-workplate-tabs',
    '[hidden]': 'tabs().length === 0',
  },
})
export class WorkplateTabs {
  private readonly router = inject(Router);
  private readonly slicer = inject(Slicer);
  private readonly names = inject(WorkplateNames);
  private readonly openWorkplates = inject(OpenWorkplates);

  readonly tabs = this.openWorkplates.tabs;
  readonly activeUuid = this.openWorkplates.activeUuid;
  readonly editingUuid = signal<string | null>(null);

  /** The stored custom name, if the tab was renamed. */
  nameFor(tab: OpenWorkplateTab): string {
    return this.names.nameFor(tab.uuid) ?? '';
  }

  /** Fallback shown as placeholder when the tab has no custom name yet. */
  placeholderFor(tab: OpenWorkplateTab): string {
    return this.names.displayNameFor(tab.uuid, tab.filename);
  }

  rename(uuid: string, event: Event): void {
    this.names.setName(uuid, (event.target as HTMLInputElement).value);
    this.editingUuid.set(null);
  }

  activate(uuid: string): void {
    if (uuid === this.activeUuid()) {
      return;
    }
    void this.router.navigate(['/slice', uuid]);
  }

  startEditing(uuid: string, event: Event): void {
    event.stopPropagation();
    this.editingUuid.set(uuid);
  }

  stopEditing(uuid: string, newName: string): void {
    if (this.editingUuid() === uuid) {
      if (newName.trim()) {
        this.names.setName(uuid, newName.trim());
      }
      this.editingUuid.set(null);
    }
  }

  onInputBlur(uuid: string, event: FocusEvent): void {
    const input = event.target as HTMLInputElement;
    this.stopEditing(uuid, input.value);
  }

  onInputKeydown(uuid: string, event: KeyboardEvent, input: HTMLInputElement): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      this.stopEditing(uuid, input.value);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      this.editingUuid.set(null);
    }
  }

  /** Discard the current workplate (file + scene) and open a fresh tab. */
  async addTab(): Promise<void> {
    await this.slicer.resetWorkplate();
    await this.router.navigate(['/slice', 'new']);
  }

  async closeTab(event: Event, uuid: string): Promise<void> {
    event.stopPropagation();
    if (uuid === this.activeUuid() && this.tabs().length === 1) {
      // Closing the only open tab is the same as discarding the plate: there
      // is nothing left to switch to, so clear the scene before navigating
      // away rather than leaving it to bleed into the "empty" plate.
      await this.slicer.resetWorkplate();
    }
    this.openWorkplates.close(uuid);
  }

  showContextMenu(event: MouseEvent, uuid: string): void {
    event.preventDefault();
    event.stopPropagation();
    const menu = document.createElement('div');
    menu.className = 'wp-context-menu';
    menu.style.position = 'fixed';
    menu.style.left = event.clientX + 'px';
    menu.style.top = event.clientY + 'px';
    menu.style.zIndex = '1000';
    menu.innerHTML = `
      <button class="wp-context-item" data-action="rename">Rename</button>
      <hr class="wp-context-divider" />
      <button class="wp-context-item" data-action="close">Close Tab</button>
    `;
    document.body.appendChild(menu);

    const handleClick = (e: Event) => {
      const target = e.target as HTMLElement;
      const action = target.getAttribute('data-action');
      if (action === 'rename') {
        this.editingUuid.set(uuid);
      } else if (action === 'close') {
        void this.closeTab(e, uuid);
      }
      document.body.removeChild(menu);
      document.removeEventListener('click', handleClick);
    };

    setTimeout(() => {
      document.addEventListener('click', handleClick);
    });
  }
}
