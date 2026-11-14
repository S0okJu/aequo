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
}

export interface Item {
  id: number;
  feedId: number;
  guidHash: string;
  title: string | null;
  link: string | null;
  summary: string | null;
  publishedAt: number;
  fetchedAt: number;
  isRead: boolean;
}
