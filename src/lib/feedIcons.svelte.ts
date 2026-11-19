// Visible consumers request bounded batches; retained data shares the app cache.
import { listen } from '@tauri-apps/api/event';
import { getFeedIcons } from '$lib/api';
import { managedCache, serializedBytes } from './cacheBudget';

const icons = $state<Record<number, string>>({});
/* eslint-disable svelte/prefer-svelte-reactivity -- Queue ownership is deliberately non-reactive; cache eviction must not schedule new fetches. */
const consumers = new Map<number, number>();
const pending = new Set<number>();
const activeIds = new Set<number>();
const invalidated = new Set<number>();
/* eslint-enable svelte/prefer-svelte-reactivity */
let owners = 0;
let generation = 0;
let inFlight = false;
let timer: ReturnType<typeof setTimeout> | undefined;
let subscription: Promise<() => void> | undefined;

function forget(id: number) {
  delete icons[id];
  managedCache.release(`icon:${id}`);
}

function schedule() {
  if (!owners || inFlight || timer !== undefined || !pending.size) return;
  timer = setTimeout(() => {
    timer = undefined;
    void load();
  }, 20);
}

async function load() {
  if (!owners || inFlight || !pending.size) return;
  const ownGeneration = generation;
  const ids = [...pending].slice(0, 32);
  for (const id of ids) activeIds.add(id);
  for (const id of ids) pending.delete(id);
  inFlight = true;
  try {
    await subscription;
    if (!owners || ownGeneration !== generation) return;
    const result = await getFeedIcons(ids);
    if (!owners || ownGeneration !== generation) return;
    for (const id of ids) {
      if (invalidated.has(id)) continue;
      const value = result[id];
      if (!value) {
        forget(id);
        continue;
      }
      if (
        managedCache.put(`icon:${id}`, serializedBytes({ [id]: value }), () => {
          delete icons[id];
        })
      )
        icons[id] = value;
    }
  } catch (error) {
    console.error('get_feed_icons failed:', error);
  } finally {
    inFlight = false;
    activeIds.clear();
    invalidated.clear();
    schedule();
  }
}

export function feedIconUrl(feedId: number): string | undefined {
  managedCache.touch(`icon:${feedId}`);
  return icons[feedId];
}

/** The component owns its interest; eviction does not trigger a reload loop. */
export function watchFeedIcon(feedId: number): () => void {
  consumers.set(feedId, (consumers.get(feedId) ?? 0) + 1);
  if (!icons[feedId]) pending.add(feedId);
  schedule();
  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    const count = (consumers.get(feedId) ?? 1) - 1;
    if (count > 0) consumers.set(feedId, count);
    else {
      consumers.delete(feedId);
      pending.delete(feedId);
      if (!pending.size && timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
    }
  };
}

/** Root-layout owned subscription with cleanup for listeners, timers and cache. */
export function startFeedIconSync(): () => void {
  owners++;
  if (owners === 1) {
    const ownGeneration = generation;
    for (const id of consumers.keys()) if (!icons[id]) pending.add(id);
    subscription = listen<{ feedId: number }>('feed-icon-updated', (event) => {
      if (!owners || ownGeneration !== generation) return;
      const id = event.payload.feedId;
      forget(id);
      if (activeIds.has(id)) invalidated.add(id);
      if (consumers.has(id)) pending.add(id);
      schedule();
    });
    void subscription.catch((error) => console.error('feed icon subscription failed:', error));
  }
  schedule();
  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    owners--;
    if (owners) return;
    void subscription?.then((stop) => stop()).catch(() => undefined);
    subscription = undefined;
    generation++;
    clearTimeout(timer);
    timer = undefined;
    pending.clear();
    for (const id of Object.keys(icons)) forget(Number(id));
  };
}
