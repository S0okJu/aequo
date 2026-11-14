// Typed wrappers around the Tauri commands (src-tauri/src/commands).
// Argument keys are camelCase - Tauri maps them to the Rust
// function's snake_case parameters automatically.

import { invoke } from '@tauri-apps/api/core';
import type { FeedSummary, Item } from './types';

export function listFeeds(): Promise<FeedSummary[]> {
  return invoke('list_feeds');
}

export function listItems(
  limit: number,
  offset: number,
  unreadOnly: boolean,
  feedId: number | null,
): Promise<Item[]> {
  return invoke('list_items', { limit, offset, unreadOnly, feedId });
}

export function markRead(itemId: number): Promise<void> {
  return invoke('mark_read', { itemId });
}

export function markFeedRead(feedId: number): Promise<void> {
  return invoke('mark_feed_read', { feedId });
}

export function addFeed(url: string): Promise<number> {
  return invoke('add_feed', { url });
}

export function refreshFeed(feedId: number): Promise<string> {
  return invoke('refresh_feed', { feedId });
}
