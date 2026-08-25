/**
 * Whether a profile matches an active label filter. Uses AND semantics — the
 * profile must carry *every* selected label — matching how issue trackers
 * narrow a list as more labels are added. An empty filter matches everything.
 */
export function matchesAllLabels(
  item: { label_ids?: string[] },
  selectedIds: readonly string[],
): boolean {
  if (selectedIds.length === 0) {
    return true;
  }
  const owned = item.label_ids ?? [];
  return selectedIds.every((id) => owned.includes(id));
}

/** Toggle a label id in a list, returning a new array (add if absent, else remove). */
export function toggledLabelIds(current: readonly string[] | undefined, id: string): string[] {
  const list = current ?? [];
  return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
}

/** Toggle an id within a filter set (same add/remove semantics). */
export function toggledFilter(current: readonly string[], id: string): string[] {
  return current.includes(id) ? current.filter((x) => x !== id) : [...current, id];
}
