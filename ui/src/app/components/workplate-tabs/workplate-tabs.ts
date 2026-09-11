import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { OpenWorkplateTab, OpenWorkplates } from '../../services/open-workplates';
import { Slicer } from '../../services/slicer';
import { WorkplateNames } from '../../services/workplate-names';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import { ContextMenuTrigger } from '../../services/context-menu/context-menu-trigger';
import type { ContextMenuItem } from '../../services/context-menu/context-menu.model';
import { Icon, IconButton, TooltipDirective } from '@coldcrabby/ui';

/**
 * Open-workplate tab strip shown in the titlebar, replacing the single
 * editable plate-name field. Each tab is an independently renamed, switchable
 * workplate (see {@link OpenWorkplates}); the `+` opens a fresh one.
 */
@Component({
  selector: 'nexus-workplate-tabs',
  imports: [Icon, IconButton, TooltipDirective, ContextMenuTrigger],
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
  private readonly contextMenu = inject(ContextMenuService);

  readonly tabs = this.openWorkplates.tabs;
  readonly activeUuid = this.openWorkplates.activeUuid;
  /** UUID of the tab whose name is currently being edited, if any. */
  readonly editingUuid = signal<string | null>(null);

  /** The stored custom name, if the tab was renamed. */
  nameFor(tab: OpenWorkplateTab): string {
    return this.names.nameFor(tab.uuid) ?? '';
  }

  /** Fallback shown as placeholder when the tab has no custom name yet. */
  placeholderFor(tab: OpenWorkplateTab): string {
    return this.names.displayNameFor(tab.uuid, tab.filename);
  }

  /**
   * Switching tabs and renaming share the same click: a tab you're not on
   * activates it, like any tab strip; clicking the one you're already on has
   * nothing left to *do* but rename it, so that's what a re-click means here.
   */
  activate(uuid: string, event: Event): void {
    if (uuid === this.activeUuid()) {
      this.startEditing(uuid, event);
      return;
    }
    void this.router.navigate(['/slice', uuid]);
  }

  startEditing(uuid: string, event?: Event): void {
    event?.preventDefault();
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

  async closeTab(uuid: string, event?: Event): Promise<void> {
    event?.stopPropagation();
    if (uuid === this.activeUuid() && this.tabs().length === 1) {
      // Closing the only open tab is the same as discarding the plate: there
      // is nothing left to switch to, so clear the scene before navigating
      // away rather than leaving it to bleed into the "empty" plate.
      await this.slicer.resetWorkplate();
    }
    this.openWorkplates.close(uuid);
  }

  /** Close every tab except `uuid`. */
  closeOthers(uuid: string): void {
    for (const tab of this.tabs()) {
      if (tab.uuid !== uuid) {
        this.openWorkplates.close(tab.uuid);
      }
    }
  }

  /** Close every open tab, clearing the scene since nothing is left on screen. */
  async closeAll(): Promise<void> {
    const all = this.tabs();
    if (all.length === 0) {
      return;
    }
    await this.slicer.resetWorkplate();
    for (const tab of all) {
      this.openWorkplates.close(tab.uuid);
    }
  }

  onContextMenu(event: MouseEvent, uuid: string): void {
    const items: ContextMenuItem[] = [
      { label: 'Rename…', icon: 'edit-pencil', action: () => this.startEditing(uuid) },
      { separator: true, label: '' },
      { label: 'Close Tab', icon: 'xmark', action: () => void this.closeTab(uuid) },
      {
        label: 'Close Other Tabs',
        action: () => this.closeOthers(uuid),
        disabled: this.tabs().length <= 1,
      },
      { label: 'Close All Tabs', action: () => void this.closeAll() },
    ];
    void this.contextMenu.open(event, items);
  }
}
