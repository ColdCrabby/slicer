import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Slicer } from '../slicer';
import { WorkplateObjects } from '../workplate-objects';
import { OpenWith } from './open-with';

/**
 * `OpenWith` is the only part of "Open with Cold Crabby" that makes a decision
 * rather than a translation, so what is pinned here is that decision — join the
 * plate on screen, never replace it — and the two routes a model reaches it by.
 */
type OpenedEvent = { payload: unknown };

const tauri = vi.hoisted(() => ({
  listen: vi.fn(
    async (_event: string, _handler: (event: { payload: unknown }) => void) => () => undefined,
  ),
  invoke: vi.fn(async (_command: string) => [] as unknown),
  readFile: vi.fn(async (_path: string) => new Uint8Array([1, 2, 3])),
}));

vi.mock('@tauri-apps/api/event', () => ({ listen: tauri.listen }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke }));
vi.mock('@tauri-apps/plugin-fs', () => ({ readFile: tauri.readFile }));

const BENCHY = { path: '/models/benchy.stl', file_name: 'benchy.stl' };
const HULL = { path: '/models/hull.3mf', file_name: 'hull.3mf' };

function setup() {
  const slicer = {
    startWorkplate: vi.fn(async (file: File) => ({ requestUuid: `plate-${file.name}` })),
  };
  const workplate = {
    objects: signal<unknown[]>([]),
    addFiles: vi.fn(async (files: readonly File[]) =>
      files.map((file) => ({ file, objectIds: [1n] })),
    ),
    queuePending: vi.fn(),
  };
  const navigate = vi.fn(async () => true);

  TestBed.configureTestingModule({
    providers: [
      { provide: Slicer, useValue: slicer },
      { provide: WorkplateObjects, useValue: workplate },
      { provide: Router, useValue: { navigate } },
    ],
  });

  return { openWith: TestBed.inject(OpenWith), slicer, workplate, navigate };
}

/** Let the service's internal promise chain settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Deliver models the way a *running* app receives them. */
async function emit(files: { path: string; file_name: string }[]): Promise<void> {
  const handler = tauri.listen.mock.calls.at(-1)?.[1] as (event: OpenedEvent) => void;
  handler({ payload: files });
  await settle();
}

describe('OpenWith', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauri.listen.mockResolvedValue(() => undefined);
    tauri.invoke.mockResolvedValue([]);
    tauri.readFile.mockResolvedValue(new Uint8Array([1, 2, 3]));
  });

  it('drains the launch buffer, so a cold-start file is not lost to the race', async () => {
    tauri.invoke.mockResolvedValue([BENCHY]);
    const { openWith, slicer, navigate } = setup();

    openWith.start();
    await settle();

    expect(tauri.invoke).toHaveBeenCalledWith('take_opened_files');
    expect(slicer.startWorkplate).toHaveBeenCalledTimes(1);
    expect(navigate).toHaveBeenCalledWith(['/slice', 'plate-benchy.stl'], { state: undefined });
  });

  it('subscribes before draining, or a file arriving in between is dropped', async () => {
    const { openWith } = setup();

    openWith.start();
    await settle();

    expect(tauri.listen.mock.invocationCallOrder[0]).toBeLessThan(
      tauri.invoke.mock.invocationCallOrder[0],
    );
  });

  it('subscribes once, however often it is started', async () => {
    const { openWith } = setup();

    openWith.start();
    openWith.start();
    await settle();

    expect(tauri.listen).toHaveBeenCalledTimes(1);
  });

  it('opens a plate when there is none', async () => {
    const { openWith, slicer, workplate, navigate } = setup();
    openWith.start();
    await settle();

    await emit([BENCHY]);

    expect(slicer.startWorkplate).toHaveBeenCalledTimes(1);
    expect(workplate.addFiles).toHaveBeenCalledTimes(0);
    expect(navigate).toHaveBeenCalledTimes(1);
  });

  it('joins the plate already on screen rather than throwing it away', async () => {
    const { openWith, slicer, workplate, navigate } = setup();
    workplate.objects.set([{ id: 1n }]);
    openWith.start();
    await settle();

    await emit([BENCHY]);

    expect(workplate.addFiles).toHaveBeenCalledTimes(1);
    expect(slicer.startWorkplate).toHaveBeenCalledTimes(0);
    expect(navigate).toHaveBeenCalledTimes(0);
  });

  it('plates the first of a batch and queues the rest, as a multi-file drop does', async () => {
    const { openWith, slicer, workplate } = setup();
    openWith.start();
    await settle();

    await emit([BENCHY, HULL]);

    expect(slicer.startWorkplate.mock.calls[0][0].name).toBe('benchy.stl');
    expect(workplate.queuePending).toHaveBeenCalledWith([
      expect.objectContaining({ name: 'hull.3mf' }),
    ]);
  });

  it('keeps the path on the file, so the slicer reads the model where it lies', async () => {
    const { openWith, slicer } = setup();
    openWith.start();
    await settle();

    await emit([BENCHY]);

    const file = slicer.startWorkplate.mock.calls[0][0] as File & { path?: string };
    expect(file.path).toBe('/models/benchy.stl');
  });

  it('survives an unreadable model without wedging the queue', async () => {
    const { openWith, slicer } = setup();
    openWith.start();
    await settle();

    tauri.readFile.mockRejectedValueOnce(new Error('permission denied'));
    await emit([BENCHY]);
    expect(slicer.startWorkplate).toHaveBeenCalledTimes(0);

    await emit([BENCHY]);
    expect(slicer.startWorkplate).toHaveBeenCalledTimes(1);
  });
});
