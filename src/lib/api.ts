// Typed wrappers around the Tauri commands (src-tauri/src/commands).
// Argument keys are camelCase - Tauri maps them to the Rust
// function's snake_case parameters automatically.

import { invoke } from '@tauri-apps/api/core';
import type {
  AiProvider,
  AiSendResult,
  AiSession,
  AiTranscript,
  Annotation,
  AnnotationKind,
  ArticleView,
  FeedSummary,
  Item,
  RetentionSettings,
  CleanupPreview,
  CleanupResult,
  FeedFolder,
} from './types';

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

export function setFeedRefreshInterval(feedId: number, intervalSecs: number): Promise<void> {
  return invoke('set_feed_refresh_interval', { feedId, intervalSecs });
}

export type ItemCursor = { publishedAt: number; id: number };

export function listItems(
  limit: number,
  cursor: ItemCursor | null,
  unreadOnly: boolean,
  feedId: number | null,
  folderId?: number | null,
  savedOnly = false,
): Promise<Item[]> {
  const args: {
    limit: number;
    cursorPublishedAt: number | null;
    cursorId: number | null;
    unreadOnly: boolean;
    feedId: number | null;
    folderId?: number | null;
    savedOnly: boolean;
  } = {
    limit,
    cursorPublishedAt: cursor?.publishedAt ?? null,
    cursorId: cursor?.id ?? null,
    unreadOnly,
    feedId,
    savedOnly,
  };
  if (folderId !== undefined) args.folderId = folderId;
  return invoke('list_items', args);
}

export function markRead(itemId: number): Promise<void> {
  return invoke('mark_read', { itemId });
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

export function markFeedRead(feedId: number): Promise<void> {
  return invoke('mark_feed_read', { feedId });
}

export function addFeed(url: string): Promise<number> {
  return invoke('add_feed', { url });
}

export function refreshFeed(feedId: number): Promise<string> {
  return invoke('refresh_feed', { feedId });
}

export function deleteFeed(feedId: number): Promise<void> {
  return invoke('delete_feed', { feedId });
}

export function listAiSessions(itemId: number, provider: AiProvider): Promise<AiSession[]> {
  return invoke('list_ai_sessions', { itemId, provider });
}

export function loadAiTranscript(sessionId: number): Promise<AiTranscript> {
  return invoke('load_ai_transcript', { sessionId });
}

export function sendAiMessage(
  itemId: number,
  provider: AiProvider,
  sessionId: number | null,
  prompt: string,
): Promise<AiSendResult> {
  return invoke('send_ai_message', { itemId, provider, sessionId, prompt });
}

export function deleteAiSession(sessionId: number): Promise<void> {
  return invoke('delete_ai_session', { sessionId });
}

export function loadArticle(itemId: number, forceRefresh = false): Promise<ArticleView> {
  return invoke('load_article', { itemId, forceRefresh });
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
