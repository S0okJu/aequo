import { beforeEach, describe, expect, it, vi } from 'vitest';
const api = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(() => true), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: api.invoke, isTauri: api.isTauri }));
vi.mock('@tauri-apps/api/event', () => ({ listen: api.listen }));
import {
  acceptUpdateSnapshot,
  startUpdateSync,
  updateAction,
  updateDraft,
  updateState,
  type UpdateSnapshot,
} from './updates.svelte';

export function snapshot(overrides: Partial<UpdateSnapshot> = {}): UpdateSnapshot {
  return {
    revision: 1,
    operationId: 1,
    currentVersion: '1.0.1',
    phase: 'idle',
    autoUpdate: true,
    enabled: true,
    lastSuccessfulCheckAt: null,
    release: null,
    downloadedBytes: 0,
    totalBytes: null,
    error: null,
    appliedVersion: null,
    pendingInstalledVersion: null,
    ...overrides,
  };
}

describe('update synchronization', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updateState.snapshot = null;
    updateState.error = null;
    updateState.pending = false;
  });
  it('does not let a delayed query or old progress replace newer state', () => {
    acceptUpdateSnapshot(snapshot({ revision: 5, operationId: 3, phase: 'restart_required' }));
    acceptUpdateSnapshot(snapshot({ revision: 4, operationId: 2, phase: 'downloading' }));
    expect(updateState.snapshot?.phase).toBe('restart_required');
  });
  it('shares a pending action instead of issuing duplicate commands', async () => {
    let finish!: (value: UpdateSnapshot) => void;
    api.invoke.mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const first = updateAction('check_for_updates');
    await updateAction('check_for_updates');
    expect(api.invoke).toHaveBeenCalledTimes(1);
    finish(snapshot({ phase: 'up_to_date' }));
    await first;
    expect(updateState.pending).toBe(false);
  });
  it('blocks restart while a composer draft exists, and clears it on cleanup', async () => {
    api.invoke.mockResolvedValue(null);
    const token = Symbol();
    updateDraft(token, true);
    await updateAction('restart_after_update');
    expect(updateState.error).toBe('draft_active');
    expect(api.invoke.mock.calls.some(([command]) => command === 'restart_after_update')).toBe(
      false,
    );
    updateDraft(token, false);
    await updateAction('restart_after_update');
    expect(api.invoke).toHaveBeenLastCalledWith('restart_after_update', undefined);
    expect(
      api.invoke.mock.calls
        .filter(([command]) => command === 'set_update_draft')
        .map(([, args]) => args),
    ).toEqual([{ dirty: true }, { dirty: false }]);
  });
  it('subscribes before querying and removes a listener even when cleanup wins the race', async () => {
    let finish!: (stop: () => void) => void;
    const stop = vi.fn();
    api.listen.mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const cleanup = startUpdateSync();
    cleanup();
    finish(stop);
    await Promise.resolve();
    expect(stop).toHaveBeenCalledOnce();
    expect(api.invoke).not.toHaveBeenCalled();
  });
});
