import type { WorkplateSetup } from '../workplate-persistence';

/** One placed object as the plate's document records it. */
export type PlacedObject = NonNullable<WorkplateSetup['objects']>[number];

/** Which instance of a file's part a saved record should be given. */
export interface Placement {
  /** The record this satisfies, in document order. */
  record: PlacedObject;
  /** Key into the parsed file's parts: `<file_id>#<part_index>`. */
  key: string;
  /**
   * Which copy of that part this is. `0` is the object the parse produced;
   * anything higher has to be duplicated from it, because the file yields each
   * part exactly once however many times the user put it on the plate.
   */
  occurrence: number;
}

/** What to do with the scene the plate's files parse into. */
export interface PlacementPlan {
  /** In document order, one per record that can be satisfied. */
  placements: Placement[];
  /** Records naming a file that could not be resolved. */
  missing: number;
  /**
   * Parts the document never claimed, as `<file_id>#<part_index>`.
   *
   * A 3MF is parsed whole, so deleting one of its parts and saving leaves an
   * object the plate does not want. Removing these is what makes a restored
   * plate the plate the user saved, rather than the files it was cut from.
   */
  spare: string[];
}

/** Key a record against the part of the file it came from. */
export function placementKey(fileId: string, partIndex: number): string {
  return `${fileId}#${partIndex}`;
}

/**
 * Work out, without touching the scene, which object each saved record gets.
 *
 * `partsByFile` is what each resolved file parsed into: its id mapped to the
 * number of parts it yields. A file the document names but that nothing could
 * resolve is simply absent, and its records count as missing — which is how a
 * restore reports honestly on a model whose bytes are gone instead of quietly
 * opening a plate with a hole in it.
 */
export function planPlacements(
  objects: readonly PlacedObject[],
  partsByFile: ReadonlyMap<string, number>,
): PlacementPlan {
  const used = new Map<string, number>();
  const placements: Placement[] = [];
  let missing = 0;

  for (const record of objects) {
    const parts = partsByFile.get(record.file_id);
    const partIndex = record.part_index ?? 0;
    if (parts === undefined || partIndex >= parts) {
      missing += 1;
      continue;
    }
    const key = placementKey(record.file_id, partIndex);
    const occurrence = used.get(key) ?? 0;
    used.set(key, occurrence + 1);
    placements.push({ record, key, occurrence });
  }

  const spare: string[] = [];
  for (const [fileId, parts] of partsByFile) {
    for (let part = 0; part < parts; part += 1) {
      const key = placementKey(fileId, part);
      if (!used.has(key)) {
        spare.push(key);
      }
    }
  }

  return { placements, missing, spare };
}
