import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { managedCache } from './cacheBudget';

const api = vi.hoisted(() => ({
  getFeedIcons: vi.fn(),
  stop: vi.fn(),
  handler: null as null | ((event: { payload: { feedId: number } }) => void),
}));
vi.mock('$lib/api', () => ({ getFeedIcons: api.getFeedIcons }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_event: string, handler: typeof api.handler) => {
    api.handler = handler;
    return api.stop;
  }),
}));
import { feedIconUrl, startFeedIconSync, watchFeedIcon } from './feedIcons.svelte';

const cleanup: (() => void)[] = [];
beforeEach(() => {
  vi.useFakeTimers();
  api.getFeedIcons.mockReset();
  api.stop.mockClear();
  managedCache.setLimit(16 * 1024 * 1024);
  api.getFeedIcons.mockImplementation(async (ids: number[]) =>
    Object.fromEntries(ids.map((id) => [id, `data:image/png;base64,${id}`])),
  );
});
afterEach(async () => {
  for (const stop of cleanup.splice(0).reverse()) stop();
  await Promise.resolve();
  vi.useRealTimers();
  expect(managedCache.bytes).toBe(0);
});

describe('feed icon cache ownership', () => {
  it('batches visible interests and releases its single listener and retained data', async () => {
    for (let id = 1; id <= 70; id++) cleanup.push(watchFeedIcon(id));
    const first = startFeedIconSync();
    const second = startFeedIconSync();
    cleanup.push(first, second);
    await vi.advanceTimersByTimeAsync(100);
    expect(api.getFeedIcons.mock.calls.map((call) => call[0].length)).toEqual([32, 32, 6]);
    expect(feedIconUrl(70)).toContain('70');
    expect(managedCache.bytes).toBeGreaterThan(0);
    first();
    await Promise.resolve();
    expect(api.stop).not.toHaveBeenCalled();
    second();
    await Promise.resolve();
    expect(api.stop).toHaveBeenCalledOnce();
    expect(feedIconUrl(70)).toBeUndefined();
  });

  it('honors the shared byte budget without repeatedly fetching evicted visible icons', async () => {
    managedCache.setLimit(1000);
    api.getFeedIcons.mockImplementation(async (ids: number[]) =>
      Object.fromEntries(ids.map((id) => [id, 'x'.repeat(650)])),
    );
    cleanup.push(watchFeedIcon(1), watchFeedIcon(2), startFeedIconSync());
    await vi.advanceTimersByTimeAsync(100);
    expect(api.getFeedIcons).toHaveBeenCalledTimes(1);
    expect(managedCache.bytes).toBeLessThanOrEqual(1000);
    expect(feedIconUrl(1)).toBeUndefined();
    expect(feedIconUrl(2)).toBeTruthy();
    managedCache.setLimit(100);
    expect(feedIconUrl(2)).toBeUndefined();
    await vi.advanceTimersByTimeAsync(100);
    expect(api.getFeedIcons).toHaveBeenCalledTimes(1);
  });

  it('does not resurrect a deleted icon from a stale in-flight response', async () => {
    let finish: (result: Record<number, string>) => void = () => {};
    api.getFeedIcons.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const unwatch = watchFeedIcon(1);
    cleanup.push(unwatch, startFeedIconSync());
    await vi.advanceTimersByTimeAsync(20);
    api.handler?.({ payload: { feedId: 1 } });
    unwatch();
    finish({ 1: 'stale icon' });
    await vi.advanceTimersByTimeAsync(100);
    expect(feedIconUrl(1)).toBeUndefined();
  });
});
