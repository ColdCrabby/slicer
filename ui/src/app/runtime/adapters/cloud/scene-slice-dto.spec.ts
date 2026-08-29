import { describe, expect, it } from 'vitest';
import { toSliceDtos } from './scene-slice-dto';

type TestObject = Parameters<typeof toSliceDtos>[0][number];

function object(name: string, sourceId: string | null, x = 0, part = 0): TestObject {
  return {
    name,
    translation: [x, 0, 0],
    euler_xyz_deg: [0, 0, 0],
    scale: [1, 1, 1],
    source_id: sourceId,
    source_part: part,
  };
}

describe('toSliceDtos', () => {
  it('resolves each object to its own uploaded file', () => {
    const dtos = toSliceDtos(
      [object('benchy.stl', 'file-a'), object('cube.stl', 'file-b')],
      ['file-a', 'file-b'],
    );

    expect(dtos.map((d) => d.file_id)).toEqual(['file-a', 'file-b']);
  });

  it('follows the object, not the upload order', () => {
    // The scene list and the upload list are maintained independently, so
    // their orders routinely diverge. Pairing them by index would send
    // file-b's bytes for the object built from file-a.
    const dtos = toSliceDtos(
      [object('cube.stl', 'file-b'), object('benchy.stl', 'file-a')],
      ['file-a', 'file-b'],
    );

    expect(dtos.map((d) => d.file_id)).toEqual(['file-b', 'file-a']);
  });

  it('gives duplicates of one model the same file', () => {
    const dtos = toSliceDtos(
      [object('cube.stl', 'file-a', 0), object('cube.stl', 'file-a', 30)],
      ['file-a'],
    );

    expect(dtos.map((d) => d.file_id)).toEqual(['file-a', 'file-a']);
    expect(dtos.map((d) => d.transform?.translation?.[0])).toEqual([0, 30]);
  });

  it('does not collapse distinct models onto the first upload', () => {
    // The regression this function exists to prevent: with more objects than
    // the caller happens to list as uploads, index-pairing fell back to
    // uploadFileIds[0] and printed the same model twice.
    const dtos = toSliceDtos(
      [object('benchy.stl', 'file-a'), object('cube.stl', 'file-b')],
      ['file-a'],
    );

    expect(dtos.map((d) => d.file_id)).toEqual(['file-a', 'file-b']);
  });

  it('carries each object transform through untouched', () => {
    const dtos = toSliceDtos(
      [
        {
          name: 'cube.stl',
          translation: [10, 20, 30],
          euler_xyz_deg: [0, 45, 90],
          scale: [1, 2, 3],
          source_id: 'file-a',
          source_part: 0,
        },
      ],
      ['file-a'],
    );

    expect(dtos[0].transform).toEqual({
      translation: [10, 20, 30],
      euler_xyz_deg: [0, 45, 90],
      scale: [1, 2, 3],
    });
  });

  it('distinguishes parts of one multi-object file', () => {
    // A 3MF holding "top" and "bottom" becomes two plate objects sharing one
    // upload. Without the part index the server would re-load the whole file
    // for each and print every part twice.
    const dtos = toSliceDtos(
      [object('top', 'file-3mf', 0, 0), object('bottom', 'file-3mf', 40, 1)],
      ['file-3mf'],
    );

    expect(dtos.map((d) => d.file_id)).toEqual(['file-3mf', 'file-3mf']);
    expect(dtos.map((d) => d.part_index)).toEqual([0, 1]);
  });

  it('keeps the part index when a multi-object file is duplicated', () => {
    const dtos = toSliceDtos(
      [object('bottom', 'file-3mf', 0, 1), object('bottom', 'file-3mf', 60, 1)],
      ['file-3mf'],
    );

    expect(dtos.map((d) => d.part_index)).toEqual([1, 1]);
  });

  it('falls back to the sole upload for an object with no source', () => {
    const dtos = toSliceDtos([object('legacy.stl', null)], ['file-a']);

    expect(dtos.map((d) => d.file_id)).toEqual(['file-a']);
  });

  it('throws rather than slicing the wrong mesh when an object is unresolvable', () => {
    expect(() => toSliceDtos([object('orphan.stl', null)], [])).toThrow(/orphan\.stl/);
  });

  it('falls back to the upload when the plate has no objects yet', () => {
    const dtos = toSliceDtos([], ['file-a']);

    expect(dtos).toHaveLength(1);
    expect(dtos[0].file_id).toBe('file-a');
  });
});
