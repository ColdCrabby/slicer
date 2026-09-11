import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  effect,
  inject,
  signal,
  viewChild,
  viewChildren,
} from '@angular/core';
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
    '[hidden]': 'tabs().length === 0 && !isNewPlate()',
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
  readonly isNewPlate = this.openWorkplates.isNewPlate;
  /** UUID of the tab whose name is currently being edited, if any. */
  readonly editingUuid = signal<string | null>(null);

  private readonly editInput = viewChild<ElementRef<HTMLInputElement>>('editInput');
  private readonly tabEls = viewChildren<ElementRef<HTMLElement>>('tabEl');

  /**
   * Width the label occupied when editing began, in px.
   *
   * The editor replaces a `<span>` sized by its text with an `<input>` sized by
   * the browser's default, so without this the tab jumped to a different width
   * the instant you double-clicked it. Held as the input's `min-width`; the
   * input grows past it from there as the name gets longer.
   */
  protected readonly editWidth = signal<number | null>(null);

  constructor() {
    // The `autofocus` attribute is honoured when the parser meets it, so an
    // input swapped in by a control-flow block is simply never focused —
    // Safari in particular ignores it entirely. Focusing here is what makes the
    // box you just opened the box you are typing in; selecting the text means
    // the common case (replace the name outright) needs no extra gesture.
    effect(() => {
      const input = this.editInput()?.nativeElement;
      if (input) {
        input.focus();
        input.select();
      }
    });
  }

  /** The stored custom name, if the tab was renamed. */
  nameFor(tab: OpenWorkplateTab): string {
    return this.names.nameFor(tab.uuid) ?? '';
  }

  /** Fallback shown as placeholder when the tab has no custom name yet. */
  placeholderFor(tab: OpenWorkplateTab): string {
    return this.names.displayNameFor(tab.uuid, tab.filename);
  }

  /**
   * A click switches tabs and nothing else.
   *
   * Renaming is a double-click (or the context menu), the way a tab strip
   * behaves everywhere else. Opening an editor on a single click meant every
   * re-click of the tab you were already on dropped a text box in your path,
   * and a click is far too cheap a gesture to start editing on.
   */
  activate(uuid: string): void {
    if (uuid === this.activeUuid()) {
      return;
    }
    void this.router.navigate(['/slice', uuid]);
  }

  startEditing(uuid: string, event?: Event): void {
    event?.preventDefault();
    // Measure before the swap — afterwards the label is gone.
    const label = this.tabEls()
      .map((ref) => ref.nativeElement)
      .find((el) => el.dataset['uuid'] === uuid)
      ?.querySelector<HTMLElement>('.wp-tab-label');
    this.editWidth.set(label ? Math.ceil(label.getBoundingClientRect().width) : null);
    this.editingUuid.set(uuid);
  }

  /**
   * Commit an edit. Clearing the box is a deliberate "use the default again",
   * not a no-op: leaving the old custom name in place made an emptied field
   * look like it had simply failed to save.
   */
  stopEditing(uuid: string, newName: string): void {
    if (this.editingUuid() !== uuid) {
      return;
    }
    // `setName` deletes the entry for a blank value, so this is both "rename"
    // and "go back to the derived name" in one call.
    this.names.setName(uuid, newName);
    this.editingUuid.set(null);
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

  /**
   * Arrow-key movement across the strip, plus Enter/Space to switch and F2 to
   * rename — what `role="tablist"` promises a keyboard user and what a strip of
   * unfocusable `div`s could not deliver. Home/End jump to the ends.
   */
  onTabKeydown(event: KeyboardEvent, uuid: string, index: number): void {
    const tabs = this.tabs();
    let next: number | null = null;
    switch (event.key) {
      case 'ArrowRight':
        next = (index + 1) % tabs.length;
        break;
      case 'ArrowLeft':
        next = (index - 1 + tabs.length) % tabs.length;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = tabs.length - 1;
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        this.activate(uuid);
        return;
      case 'F2':
        event.preventDefault();
        this.startEditing(uuid);
        return;
      default:
        return;
    }
    event.preventDefault();
    this.tabEls()[next]?.nativeElement.focus();
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
