import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { BrowserStorage } from '../../services/browser-storage';
import { SceneEngine, type SceneObjectSnapshot } from '../../services/scene-engine';
import { isSecondaryClick, isToggleClick, ViewerControl } from '../../services/viewer-control';
import { Viewport } from '../../services/viewport';
import { MODEL_FILE_ACCEPT } from '../../services/model-source';
import { objectMenuItems, WorkplateObjects } from '../../services/workplate-objects';
import { Icon, TooltipDirective } from '@coldcrabby/ui';
import { ContextMenuService } from '../../services/context-menu/context-menu.service';
import { ContextMenuTrigger } from '../../services/context-menu/context-menu-trigger';

/** Remembers whether the user folded the list away, per device. */
const EXPANDED_KEY = 'plate.objectsPanelExpanded';

/** One row in the list, with its selection and placement state resolved. */
interface ObjectRow {
  id: bigint;
  key: string;
  name: string;
  triangleCount: number;
  /** Bounding-box size in mm, rounded for display. */
  size: [number, number, number];
  selected: boolean;
  outOfBounds: boolean;
  collides: boolean;
}

/**
 * Lists every object on the workplate and lets the user manage them.
 *
 * A plate that can hold several models needs somewhere to see them: which
 * objects are there, which one is selected, and which ones cannot print where
 * they sit. Selecting a row drives the same selection the 3D gizmo and the
 * transform panel already use, so the list and the viewport never disagree.
 *
 * Placing objects is deliberately *not* here — that is one command owned by
 * the toolbar's placement control, so there is a single place to run it and a
 * single place to configure it.
 *
 * Deletion follows the two-step inline confirm the design language asks for
 * rather than a blocking modal.
 */
@Component({
  selector: 'nexus-objects-panel',
  standalone: true,
  imports: [Icon, TooltipDirective, ContextMenuTrigger],
  templateUrl: './objects-panel.html',
  styleUrl: './objects-panel.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ObjectsPanel {
  private readonly workplate = inject(WorkplateObjects);
  private readonly sceneEngine = inject(SceneEngine);
  private readonly viewerControl = inject(ViewerControl);
  private readonly viewport = inject(Viewport);
  private readonly storage = inject(BrowserStorage);
  private readonly contextMenu = inject(ContextMenuService);

  protected readonly modelFileAccept = MODEL_FILE_ACCEPT;

  /** True while picked models are being added. */
  protected readonly adding = signal(false);

  /** Id awaiting delete confirmation, if any. */
  protected readonly pendingDelete = signal<bigint | null>(null);

  /**
   * Whether the list may be folded away to its header.
   *
   * On a large desktop scene the panel costs a corner and is worth having open;
   * anywhere tighter the same list covers a third of the plate it describes,
   * and on a touch device there is no cursor to move away from it. So compact
   * viewports and tablets get a header that collapses — the object count stays
   * visible, the rows come back on tap — while a roomy desktop keeps the list
   * open with no extra control to explain.
   */
  protected readonly collapsible = computed(() => this.viewport.isCompact());

  private readonly storedExpanded = this.storage.get(EXPANDED_KEY, 'local');

  /**
   * Whether the object rows are showing. Always true where there is room.
   *
   * Folded is the default wherever it can fold: the complaint the fold answers
   * is that the list is in the way, so it starts out of the way. The header
   * keeps the count and the warning flag, so a plate that cannot print still
   * says so. The choice is remembered per device.
   */
  protected readonly expanded = computed(
    () => !this.collapsible() || this.storedExpanded() === 'true',
  );

  protected toggleExpanded(): void {
    this.storage.write(EXPANDED_KEY, this.expanded() ? 'false' : 'true', 'local');
  }

  /**
   * Hidden in G-code preview: the list describes the *model* plate, and none
   * of its actions (select, duplicate, delete) mean anything once the view
   * has switched to sliced toolpaths.
   */
  protected readonly visible = computed(
    () => this.viewerControl.viewMode() === 'model' && this.sceneEngine.objects().length > 0,
  );

  protected readonly rows = computed<ObjectRow[]>(() => {
    const selected = new Set(this.viewerControl.selectedObjectIds());
    const objects = this.sceneEngine.objects();
    // Duplicates share their file's name, so a list of three "benchy.stl" rows
    // told the user nothing about which was which. Number the repeats, in
    // plate order, so each row names one object.
    const total = new Map<string, number>();
    for (const object of objects) {
      total.set(object.name, (total.get(object.name) ?? 0) + 1);
    }
    const seen = new Map<string, number>();
    return objects.map((object) => {
      const nth = (seen.get(object.name) ?? 0) + 1;
      seen.set(object.name, nth);
      return {
        id: object.id,
        key: object.id.toString(),
        name: (total.get(object.name) ?? 1) > 1 ? `${object.name} (${nth})` : object.name,
        triangleCount: object.triangle_count,
        size: sizeOf(object),
        selected: selected.has(object.id),
        outOfBounds: object.out_of_bounds,
        collides: object.collides,
      };
    });
  });

  protected readonly count = computed(() => this.rows().length);

  /** Summary of what is wrong with the plate, or null when it is printable. */
  protected readonly warning = computed<string | null>(() => {
    const rows = this.rows();
    const offBed = rows.filter((r) => r.outOfBounds).length;
    const overlapping = rows.filter((r) => r.collides).length;
    const parts: string[] = [];
    if (offBed > 0) {
      parts.push(`${offBed} outside the build area`);
    }
    if (overlapping > 0) {
      parts.push(`${overlapping} overlapping`);
    }
    return parts.length > 0 ? parts.join(' · ') : null;
  });

  /** Add the picked models to the plate, alongside everything already on it. */
  protected async onAddFiles(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    // Reset immediately so picking the same file twice still fires a change.
    input.value = '';
    if (files.length === 0) {
      return;
    }
    this.adding.set(true);
    try {
      await this.workplate.addFilesWithFeedback(files);
    } finally {
      this.adding.set(false);
    }
  }

  /** The row a Shift-click extends from: the last one clicked without Shift. */
  private rangeAnchor: bigint | null = null;

  /**
   * Select a row the way a file list does.
   *
   * - **Click** — just this object.
   * - **⌘-click** (Ctrl off Apple platforms) — toggle it in or out.
   * - **Shift-click** — every row from the last one clicked to this one.
   *
   * Multi-select mode stands in for ⌘ on touch, so a tapped row behaves like a
   * tapped model. ⌃-click on a Mac is the context menu and never selects.
   */
  protected select(row: ObjectRow, event: Event): void {
    // Angular types `(keydown.enter)` as a plain Event, so narrow rather than
    // assume the modifier keys are present.
    const keys = event instanceof MouseEvent || event instanceof KeyboardEvent ? event : null;
    if (event instanceof MouseEvent && isSecondaryClick(event)) {
      return;
    }
    const current = this.viewerControl.selectedObjectIds();
    if (keys?.shiftKey && this.rangeAnchor !== null) {
      const ids = this.rows().map((r) => r.id);
      const from = ids.indexOf(this.rangeAnchor);
      const to = ids.indexOf(row.id);
      if (from !== -1 && to !== -1) {
        const range = ids.slice(Math.min(from, to), Math.max(from, to) + 1);
        // A range adds to a ⌘-built selection rather than replacing it, as in
        // Finder: ⌘-pick a few, then Shift-click to sweep in a run.
        const kept = isToggleClick(keys) ? current.filter((id) => !range.includes(id)) : [];
        this.viewerControl.selectedObjectIds.set([...kept, ...range]);
        return;
      }
    }
    this.rangeAnchor = row.id;
    const toggle = this.viewerControl.additiveSelection() || (keys !== null && isToggleClick(keys));
    if (!toggle) {
      this.viewerControl.selectedObjectIds.set([row.id]);
      return;
    }
    this.viewerControl.selectedObjectIds.set(
      current.includes(row.id) ? current.filter((id) => id !== row.id) : [...current, row.id],
    );
  }

  /** Double-click a row to find its object on the plate. */
  protected zoomTo(row: ObjectRow): void {
    this.viewerControl.frameObjects([row.id]);
  }

  protected duplicate(row: ObjectRow, event: Event): void {
    event.stopPropagation();
    // Select the copy, as ⌘D does: the next thing done is usually moving it.
    this.viewerControl.selectedObjectIds.set(this.workplate.duplicateAll([row.id]));
  }

  protected requestDelete(row: ObjectRow, event: Event): void {
    event.stopPropagation();
    // First click arms the confirm; a second one on the same row deletes.
    if (this.pendingDelete() === row.id) {
      this.workplate.remove(row.id);
      this.pendingDelete.set(null);
      this.viewerControl.selectedObjectIds.update((ids) => ids.filter((id) => id !== row.id));
      return;
    }
    this.pendingDelete.set(row.id);
  }

  /**
   * The same menu the 3D scene offers for the same object — acting on the whole
   * selection when the row is part of it, just as a right-click on the model
   * does.
   */
  protected onContextMenu(event: MouseEvent, row: ObjectRow): void {
    const selected = this.viewerControl.selectedObjectIds();
    const targets = selected.includes(row.id) ? [...selected] : [row.id];
    void this.contextMenu.open(event, objectMenuItems(targets, this.workplate, this.viewerControl));
  }

  protected cancelDelete(event: Event): void {
    event.stopPropagation();
    this.pendingDelete.set(null);
  }
}

/** Bounding-box dimensions in mm, rounded to one decimal. */
function sizeOf(object: SceneObjectSnapshot): [number, number, number] {
  const [min, max] = object.world_aabb;
  const round = (v: number) => Math.round(v * 10) / 10;
  return [round(max[0] - min[0]), round(max[1] - min[1]), round(max[2] - min[2])];
}
