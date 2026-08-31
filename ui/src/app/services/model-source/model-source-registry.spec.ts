import { describe, expect, it } from 'vitest';
import { ModelSourceRegistry, isSupportedModelFile, modelFormatOf } from './model-source-registry';

const CUBE = new Uint8Array([1, 2, 3, 4]);
const SPHERE = new Uint8Array([9, 9, 9]);

describe('modelFormatOf', () => {
  it('reads the extension, case-insensitively', () => {
    expect(modelFormatOf('part.STL')).toBe('stl');
    expect(modelFormatOf('scene.3mf')).toBe('3mf');
    expect(modelFormatOf('mesh.obj')).toBe('obj');
  });

  it('falls back to STL for anything unrecognised', () => {
    expect(modelFormatOf('notes.txt')).toBe('stl');
    expect(modelFormatOf('noextension')).toBe('stl');
  });

  it('is not fooled by a dot in the directory name', () => {
    expect(modelFormatOf('v1.2/model.3mf')).toBe('3mf');
  });
});

describe('isSupportedModelFile', () => {
  it('accepts the three loadable formats and nothing else', () => {
    expect(isSupportedModelFile('a.stl')).toBe(true);
    expect(isSupportedModelFile('a.obj')).toBe(true);
    expect(isSupportedModelFile('a.3mf')).toBe(true);
    expect(isSupportedModelFile('a.gcode')).toBe(false);
  });
});

describe('ModelSourceRegistry', () => {
  it('mints a distinct handle per file so two models never collide', () => {
    const registry = new ModelSourceRegistry();
    const cube = registry.register({ fileName: 'cube.stl', bytes: CUBE });
    const sphere = registry.register({ fileName: 'sphere.stl', bytes: SPHERE });

    expect(cube.sourceId).not.toBe(sphere.sourceId);
    expect(registry.get(cube.sourceId)?.bytes).toBe(CUBE);
    expect(registry.get(sphere.sourceId)?.bytes).toBe(SPHERE);
  });

  it('keeps the caller-supplied id, so an upload uuid survives', () => {
    const registry = new ModelSourceRegistry();
    const source = registry.register({
      sourceId: 'upload-uuid',
      fileName: 'cube.stl',
      bytes: CUBE,
    });

    expect(source.sourceId).toBe('upload-uuid');
    expect(registry.has('upload-uuid')).toBe(true);
  });

  it('infers the format from the filename', () => {
    const registry = new ModelSourceRegistry();
    expect(registry.register({ fileName: 'scene.3mf', bytes: CUBE }).format).toBe('3mf');
  });

  it('merges a later registration instead of dropping what it already knows', () => {
    // The desktop runtime learns a file's native path only when it first
    // slices; that must not discard the bytes the viewer loaded from.
    const registry = new ModelSourceRegistry();
    const { sourceId } = registry.register({ fileName: 'cube.stl', bytes: CUBE });

    registry.register({ sourceId, fileName: 'cube.stl', filePath: '/tmp/cube.stl' });

    const merged = registry.get(sourceId);
    expect(merged?.bytes).toBe(CUBE);
    expect(merged?.filePath).toBe('/tmp/cube.stl');
  });

  it('attaches a path to an existing entry', () => {
    const registry = new ModelSourceRegistry();
    const { sourceId } = registry.register({ fileName: 'cube.stl', bytes: CUBE });

    registry.attachFilePath(sourceId, '/cache/cube.stl');

    expect(registry.get(sourceId)?.filePath).toBe('/cache/cube.stl');
    expect(registry.get(sourceId)?.bytes).toBe(CUBE);
  });

  it('ignores a path attached to a file it does not know', () => {
    const registry = new ModelSourceRegistry();
    registry.attachFilePath('never-registered', '/cache/ghost.stl');
    expect(registry.get('never-registered')).toBeUndefined();
  });

  it('resolves nothing for a null or unknown handle rather than guessing', () => {
    // Guessing is the bug this registry exists to prevent: falling back to
    // "the first file" silently slices the wrong model.
    const registry = new ModelSourceRegistry();
    registry.register({ fileName: 'cube.stl', bytes: CUBE });

    expect(registry.get(null)).toBeUndefined();
    expect(registry.get(undefined)).toBeUndefined();
    expect(registry.get('nope')).toBeUndefined();
    expect(registry.has(null)).toBe(false);
  });

  it('forgets one file without disturbing the others', () => {
    const registry = new ModelSourceRegistry();
    const cube = registry.register({ fileName: 'cube.stl', bytes: CUBE });
    const sphere = registry.register({ fileName: 'sphere.stl', bytes: SPHERE });

    registry.forget(cube.sourceId);

    expect(registry.has(cube.sourceId)).toBe(false);
    expect(registry.has(sphere.sourceId)).toBe(true);
  });

  it('clears the whole plate', () => {
    const registry = new ModelSourceRegistry();
    const cube = registry.register({ fileName: 'cube.stl', bytes: CUBE });
    const sphere = registry.register({ fileName: 'sphere.stl', bytes: SPHERE });

    registry.clear();

    expect(registry.has(cube.sourceId)).toBe(false);
    expect(registry.has(sphere.sourceId)).toBe(false);
  });
});
