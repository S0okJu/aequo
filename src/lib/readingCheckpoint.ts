import type { ItemCursor } from './api';

export interface ListPosition {
  id: number;
  offset: number;
}
export interface ReadingCheckpoint {
  feedId: number | null;
  folderId: number | null;
  today: boolean;
  starred: boolean;
  unreadOnly: boolean;
  query: string;
  selectedItemId: number | null;
  list: (ListPosition & ItemCursor) | null;
  readerScrollTop: number;
  sidebarScrollTop: number;
  readerWidth: number;
  aiWidth: number;
}

// One route handoff, not a cache of posts, HTML, annotations or old screens.
let saved: ReadingCheckpoint | null = null;
export function saveReadingCheckpoint(value: ReadingCheckpoint): void {
  saved = structuredClone(value);
}
export function takeReadingCheckpoint(): ReadingCheckpoint | null {
  const value = saved;
  saved = null;
  return value;
}
