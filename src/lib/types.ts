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

export interface AiSendResult {
  sessionId: number;
  providerSessionId: string;
  assistantMessage: AiMessage;
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
