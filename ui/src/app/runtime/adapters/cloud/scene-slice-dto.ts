import type { SceneObjectSliceDto } from '../../../../generated/slicer-engine-ws-client-message-v1';
import type { RuntimeSceneObject } from '../../domain/scene-commands';

/**
 * Pair every scene object with the uploaded file it was loaded from.
 *
 * The server slices by `file_id`, so this mapping decides which bytes each
 * object contributes. Objects carry their own `source_id` (stamped when they
 * were added), which is what makes a plate holding several *different* models
 * sliceable: the alternative — pairing the object list against the upload list
 * by index — silently slices the wrong mesh the moment the two lists differ in
 * order or length, and collapses to "every object is the first upload" when a
 * plate has more objects than uploads.
 *
 * Objects without a `source_id` predate that plumbing and fall back to the
 * sole upload, which is only ever correct for a single-model plate — the one
 * case that can produce them.
 *
 * @throws when an object cannot be resolved to any uploaded file.
 */
export function toSliceDtos(
  objects: readonly Pick<
    RuntimeSceneObject,
    'name' | 'translation' | 'euler_xyz_deg' | 'scale' | 'source_id' | 'source_part'
  >[],
  uploadFileIds: readonly string[],
): SceneObjectSliceDto[] {
  if (objects.length === 0) {
    if (!uploadFileIds[0]) {
      throw new Error('Nothing to slice: the plate has no objects and no uploaded files.');
    }
    return [
      {
        file_id: uploadFileIds[0],
        part_index: 0,
        transform: {
          translation: [0, 0, 0],
          euler_xyz_deg: [0, 0, 0],
          scale: [1, 1, 1],
        },
      },
    ];
  }

  return objects.map((object) => {
    const fileId = object.source_id ?? uploadFileIds[0];
    if (!fileId) {
      throw new Error(`Scene object "${object.name}" has no uploaded file to slice from.`);
    }
    return {
      file_id: fileId,
      // Which object inside that file — a 3MF backs several plate objects,
      // so the file id alone does not identify the geometry.
      part_index: object.source_part ?? 0,
      transform: {
        translation: object.translation,
        euler_xyz_deg: object.euler_xyz_deg,
        scale: object.scale,
      },
    };
  });
}
