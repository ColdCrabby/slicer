import type { ContextMenuItem } from '../context-menu/context-menu.model';
import type { ViewerControl } from '../viewer-control';
import type { WorkplateObjects } from './workplate-objects';

/**
 * What a context menu offers for a set of objects — the same items whether it
 * was opened on a model in the scene or on its row in the objects list.
 *
 * The two menus used to be written separately and had drifted: the list's
 * acted on one row even inside a multi-object selection, and centred nothing
 * as a group. `targets` is already resolved by the caller: the whole selection
 * when the press landed inside it, otherwise just the object pressed.
 */
export function objectMenuItems(
  targets: readonly bigint[],
  workplate: WorkplateObjects,
  viewerControl: ViewerControl,
): ContextMenuItem[] {
  const suffix = targets.length > 1 ? ` (${targets.length})` : '';
  return [
    {
      label: `Duplicate${suffix}`,
      icon: 'copy',
      // The copies become the selection, so the next drag moves them rather
      // than the originals they were stamped from.
      action: () => viewerControl.selectedObjectIds.set(workplate.duplicateAll(targets)),
    },
    {
      label: `Drop to floor${suffix}`,
      icon: 'download',
      action: () => workplate.dropToFloor(targets),
    },
    {
      label: 'Centre on bed',
      icon: 'frame-alt',
      action: () => workplate.centerOnBed(targets),
    },
    {
      label: 'Zoom to',
      icon: 'zoom-in',
      action: () => viewerControl.frameObjects(targets),
    },
    { label: '', separator: true },
    {
      label: `Remove${suffix}`,
      icon: 'bin',
      danger: true,
      action: () => {
        workplate.removeAll(targets);
        viewerControl.selectedObjectIds.update((ids) => ids.filter((id) => !targets.includes(id)));
      },
    },
  ];
}
