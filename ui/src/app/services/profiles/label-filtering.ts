/**
 * Whether a profile matches an active label filter.
 *
 * **OR semantics** — the profile must carry *any* of the selected labels. This
 * is a shelf of things to pick from, not a search over one: a user selecting
 * `PLA` and then `PETG` is asking to see both, and requiring every label meant
 * the second click emptied the list unless something happened to carry the pair.
 * Labels here name what a profile *is*, and a profile is rarely two things at
 * once.
 *
 * An empty filter matches everything.
 */
export function matchesAnyLabel(
  item: { label_ids?: string[] },
  selectedIds: readonly string[],
): boolean {
  if (selectedIds.length === 0) {
    return true;
  }
  const owned = item.label_ids ?? [];
  return selectedIds.some((id) => owned.includes(id));
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
