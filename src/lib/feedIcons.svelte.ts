// Feed icons (docs/UI_SIMPLIFICATION_PLAN.md §3.12): loaded once from the
// backend as data: URIs and reloaded whenever the background sync stores a
// new one (`feed-icon-updated`). Reading `feedIconUrl()` in a template is
// reactive, so icons appear as they arrive.

import { listen } from '@tauri-apps/api/event';
import { getFeedIcons } from '$lib/api';

const icons = $state<Record<number, string>>({});
let started = false;

async function load() {
  try {
    Object.assign(icons, await getFeedIcons());
  } catch (error) {
    // Icons are decorative; the letter fallback covers a failed load.
    console.error('get_feed_icons failed:', error);
  }
}

export function feedIconUrl(feedId: number): string | undefined {
  return icons[feedId];
}

/** Loads icons and subscribes to updates; safe to call more than once. */
export function startFeedIconSync(): void {
  if (started) return;
  started = true;
  load();
  listen('feed-icon-updated', load);
}
