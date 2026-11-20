// Typed wrappers around the Tauri commands (src-tauri/src/commands).
// Argument keys are camelCase - Tauri maps them to the Rust
// function's snake_case parameters automatically.

import { invoke } from '@tauri-apps/api/core';
import type {
  AiProvider,
  AiProviderStatus,
  AiStart,
  AiSession,
  AiTranscript,
  Annotation,
  AnnotationKind,
  ArticleView,
  FeedSummary,
  Item,
  ItemFilter,
  RetentionSettings,
  CleanupPreview,
  CleanupResult,
  FeedFolder,
  FeedRawContent,
  ResourceSettings,
  ResourceStatus,
} from './types';

export function getResourceSettings(): Promise<ResourceStatus> {
  return invoke('get_resource_settings');
}

export function setResourceSettings(
  expectedRevision: number,
  settings: ResourceSettings,
): Promise<ResourceStatus> {
  return invoke('set_resource_settings', { expectedRevision, settings });
}

export function setResourceVisibility(hidden: boolean): Promise<void> {
  return invoke('set_resource_visibility', { hidden });
}

export function listFeeds(): Promise<FeedSummary[]> {
  return invoke('list_feeds');
}

export function listFeedFolders(): Promise<FeedFolder[]> {
  return invoke('list_feed_folders');
}

export function createFeedFolder(name: string): Promise<number> {
  return invoke('create_feed_folder', { name });
}

export function renameFeedFolder(folderId: number, name: string): Promise<void> {
  return invoke('rename_feed_folder', { folderId, name });
}

export function deleteFeedFolder(folderId: number): Promise<void> {
  return invoke('delete_feed_folder', { folderId });
}

export function updateFeedOrganization(
  feedId: number,
  folderId: number | null,
  hiddenFromAll: boolean,
): Promise<void> {
  return invoke('update_feed_organization', { feedId, folderId, hiddenFromAll });
}

/** Rewrites feed order within one folder (or the unfiled group, `folderId: null`). */
export function reorderFeedsInFolder(folderId: number | null, feedIds: number[]): Promise<void> {
  return invoke('reorder_feeds_in_folder', { folderId, feedIds });
}

export function setFeedPinned(feedId: number, pinned: boolean): Promise<void> {
  return invoke('set_feed_pinned', { feedId, pinned });
}

/** Sets the feed's display name; null or blank restores its own title. */
export function renameFeed(feedId: number, title: string | null): Promise<void> {
  return invoke('rename_feed', { feedId, title });
}

/** Whether each AI CLI is installed (not whether the user is signed in). */
export function checkAiProviders(): Promise<AiProviderStatus[]> {
  return invoke('check_ai_providers');
}

export function setFeedRefreshInterval(feedId: number, intervalSecs: number): Promise<void> {
  return invoke('set_feed_refresh_interval', { feedId, intervalSecs });
}

/** Stored feed icons as `data:` URIs keyed by feed id (fetched in the background). */
export function getFeedIcons(feedIds: number[]): Promise<Record<number, string>> {
  return invoke('get_feed_icons', { feedIds });
}

/** The feed's raw document as of its last successful poll, if it has had one. */
export function getFeedRawContent(feedId: number): Promise<FeedRawContent | null> {
  return invoke('get_feed_raw_content', { feedId });
}

export type ItemCursor = { publishedAt: number; id: number };

export function countUnreadItems(filter: ItemFilter): Promise<number> {
  return invoke('count_unread_items', { filter });
}

export function listItems(
  filter: ItemFilter,
  limit: number,
  cursor: ItemCursor | null = null,
  direction: 'older' | 'newer' = 'older',
): Promise<Item[]> {
  return invoke('list_items', {
    filter,
    limit,
    cursorPublishedAt: cursor?.publishedAt ?? null,
    cursorId: cursor?.id ?? null,
    newer: direction === 'newer',
  });
}

export function markRead(itemId: number): Promise<void> {
  return invoke('mark_read', { itemId });
}

/**
 * Marks every unread item in the view described by `filter` as read, limited
 * to items already fetched at `fetchedBefore` (unix seconds, when the view
 * was loaded). Resolves to the affected ids, for undo via `markItemsUnread`.
 */
export function markItemsRead(filter: ItemFilter, fetchedBefore: number): Promise<number[]> {
  return invoke('mark_items_read', { filter, fetchedBefore });
}

export function markItemsUnread(itemIds: number[]): Promise<number> {
  return invoke('mark_items_unread', { itemIds });
}

export function setItemSaved(itemId: number, saved: boolean): Promise<void> {
  return invoke('set_item_saved', { itemId, saved });
}

export function getRetentionSettings(): Promise<RetentionSettings> {
  return invoke('get_retention_settings');
}

export function setRetentionSettings(settings: RetentionSettings): Promise<void> {
  return invoke('set_retention_settings', { settings });
}

export function previewRetentionCleanup(): Promise<CleanupPreview> {
  return invoke('preview_retention_cleanup');
}

export function runRetentionCleanup(): Promise<CleanupResult> {
  return invoke('run_retention_cleanup');
}

export function addFeed(url: string): Promise<number> {
  return invoke('add_feed', { url });
}

export function refreshFeed(feedId: number): Promise<string> {
  return invoke('refresh_feed', { feedId });
}

/** Bound outstanding IPC requests too; the backend enforces the live policy. */
export async function refreshFeedBatch(feedIds: number[]): Promise<{ failed: number }> {
  let next = 0;
  let failed = 0;
  async function worker() {
    while (next < feedIds.length) {
      const id = feedIds[next++];
      try {
        await refreshFeed(id);
      } catch {
        failed++;
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(4, feedIds.length) }, () => worker()));
  return { failed };
}

export function deleteFeed(feedId: number): Promise<void> {
  return invoke('delete_feed', { feedId });
}

export function listAiSessions(itemId: number, provider: AiProvider): Promise<AiSession[]> {
  return invoke('list_ai_sessions', { itemId, provider });
}

export function loadAiTranscript(
  sessionId: number,
  cursor: { before?: number; after?: number } = {},
): Promise<AiTranscript> {
  return invoke('load_ai_transcript', {
    sessionId,
    before: cursor.before ?? null,
    after: cursor.after ?? null,
  });
}

/**
 * Starts an AI run and resolves at once with its request id; the answer
 * arrives as `ai-stream` events (see `AiStreamEvent`).
 */
export function startAiMessage(
  contextItemIds: number[],
  provider: AiProvider,
  sessionId: number | null,
  prompt: string,
  scopeLabel: string | null = null,
): Promise<AiStart> {
  return invoke('start_ai_message', { contextItemIds, provider, sessionId, prompt, scopeLabel });
}

/** Stops a run; it ends with a `cancelled` event carrying the text so far. */
export function cancelAiMessage(requestId: string): Promise<void> {
  return invoke('cancel_ai_message', { requestId });
}

export function deleteAiSession(sessionId: number): Promise<void> {
  return invoke('delete_ai_session', { sessionId });
}

export function getItemSummary(itemId: number): Promise<Item | null> {
  return invoke('get_item_summary', { itemId });
}

export function loadArticle(
  itemId: number,
  forceRefresh: boolean,
  requestRevision: number,
): Promise<ArticleView> {
  return invoke('load_article', { itemId, forceRefresh, requestRevision });
}

export function cancelArticleRead(requestRevision: number): Promise<void> {
  return invoke('cancel_article_read', { requestRevision });
}

export function listItemAnnotations(itemId: number): Promise<Annotation[]> {
  return invoke('list_item_annotations', { itemId });
}

export function createAnnotation(params: {
  itemId: number;
  kind: AnnotationKind;
  contentHash: string;
  startOffset: number;
  endOffset: number;
  quotedText: string;
  color?: string | null;
  note?: string | null;
}): Promise<Annotation> {
  return invoke('create_annotation', {
    itemId: params.itemId,
    kind: params.kind,
    contentHash: params.contentHash,
    startOffset: params.startOffset,
    endOffset: params.endOffset,
    quotedText: params.quotedText,
    color: params.color ?? null,
    note: params.note ?? null,
  });
}

export function updateAnnotation(params: {
  id: number;
  kind: AnnotationKind;
  contentHash: string;
  startOffset: number;
  endOffset: number;
  quotedText: string;
  color?: string | null;
  note?: string | null;
}): Promise<Annotation> {
  return invoke('update_annotation', {
    id: params.id,
    kind: params.kind,
    contentHash: params.contentHash,
    startOffset: params.startOffset,
    endOffset: params.endOffset,
    quotedText: params.quotedText,
    color: params.color ?? null,
    note: params.note ?? null,
  });
}

export function deleteAnnotation(id: number): Promise<void> {
  return invoke('delete_annotation', { id });
}

export function listAllAiSessions(provider: AiProvider | null = null): Promise<AiSession[]> {
  return invoke('list_all_ai_sessions', { provider });
}
export function latestAiSessionForItem(itemId: number): Promise<AiSession | null> {
  return invoke('latest_ai_session_for_item', { itemId });
}
export function addAiContextItems(sessionId: number, itemIds: number[]): Promise<AiSession> {
  return invoke('add_ai_context_items', { sessionId, itemIds });
}
export function removeAiContextItem(sessionId: number, position: number): Promise<AiSession> {
  return invoke('remove_ai_context_item', { sessionId, position });
}
