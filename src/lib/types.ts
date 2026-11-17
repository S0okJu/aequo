// Mirrors the camelCase JSON the Rust commands serialize (see
// src-tauri/src/db/models.rs and src-tauri/src/commands/mod.rs).

export interface FeedSummary {
  id: number;
  url: string;
  title: string | null;
  format: string | null;
  etag: string | null;
  lastModified: string | null;
  intervalSecs: number;
  nextFetchAt: number;
  lastError: string | null;
  consecutiveFailures: number;
  createdAt: number;
  unreadCount: number;
  folderId: number | null;
  hiddenFromAll: boolean;
  /** Declared language tag from the feed (e.g. "ko", "en-us"), if any. */
  language: string | null;
  /** The user's display name for the feed, if they set one. */
  customTitle: string | null;
}

export interface FeedFolder {
  id: number;
  name: string;
  sortOrder: number;
  createdAt: number;
}

export interface Item {
  id: number;
  feedId: number;
  guidHash: string;
  title: string | null;
  link: string | null;
  summary: string | null;
  fullContent: string | null;
  publishedAt: number;
  fetchedAt: number;
  isRead: boolean;
  readAt: number | null;
  isSaved: boolean;
}

/**
 * Which items a timeline view covers (mirrors `db::ItemFilter`). Shared by
 * listing and "mark all read" so both act on exactly the same rows.
 */
export interface ItemFilter {
  unreadOnly?: boolean;
  savedOnly?: boolean;
  feedId?: number | null;
  folderId?: number | null;
  /** Inclusive lower / exclusive upper bound on publishedAt (unix seconds). */
  publishedFrom?: number | null;
  publishedTo?: number | null;
  /** Search text matched against title, summary and RSS body. */
  query?: string | null;
}

/** What a sidebar action (mark read, open settings) applies to. */
export type SidebarScope = { feedId: number } | { folderId: number };

export interface RetentionSettings {
  readRetentionDays: number;
  fullTextRetentionDays: number;
}

export interface CleanupPreview {
  articlesToRemove: number;
  fullTextsToClear: number;
  estimatedBytesToFree: number;
}

export interface CleanupResult {
  articlesRemoved: number;
  fullTextsCleared: number;
  bytesFreed: number;
}

export type AiProvider = 'codex' | 'claude';

export interface AiSession {
  id: number;
  itemId: number;
  provider: AiProvider;
  providerSessionId: string | null;
  title: string;
  createdAt: number;
  updatedAt: number;
}

export interface AiMessage {
  id: number;
  sessionId: number;
  role: 'user' | 'assistant';
  content: string;
  createdAt: number;
}

export interface AiProviderStatus {
  provider: AiProvider;
  installed: boolean;
  version: string | null;
  error: string | null;
}

export interface AiStart {
  requestId: string;
  sessionId: number;
}

/**
 * Payload of the `ai-stream` event: `delta` appends `text`, `replace` sets the
 * answer so far, `status` is a progress step ("thinking" | "tool"), and
 * `done` / `error` / `cancelled` end the run (`text` = final answer / error
 * message / partial answer).
 */
export interface AiStreamEvent {
  requestId: string;
  sessionId: number;
  kind: 'delta' | 'replace' | 'status' | 'done' | 'error' | 'cancelled';
  text: string;
}

export interface AiTranscript {
  available: boolean;
  messages: AiMessage[];
  note: string | null;
}

export type AnnotationKind = 'highlight' | 'memo';

export interface Annotation {
  id: number;
  itemId: number;
  kind: AnnotationKind;
  contentHash: string;
  startOffset: number;
  endOffset: number;
  quotedText: string;
  color: string | null;
  note: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface ArticleView {
  itemId: number;
  originalUrl: string | null;
  finalUrl: string | null;
  title: string | null;
  contentHtml: string | null;
  rssContentHtml: string | null;
  textContent: string | null;
  fetchedAt: number | null;
  cached: boolean;
  error: string | null;
}
