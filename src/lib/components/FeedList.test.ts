import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import FeedList from './FeedList.svelte';
import type { FeedSummary } from '$lib/types';

const feeds: FeedSummary[] = [
  {
    id: 1,
    url: 'https://one.example/feed.xml',
    title: 'One',
    format: 'rss',
    etag: null,
    lastModified: null,
    intervalSecs: 1800,
    nextFetchAt: 0,
    lastError: null,
    consecutiveFailures: 0,
    createdAt: 0,
    unreadCount: 2,
    folderId: null,
    hiddenFromAll: false,
  },
  {
    id: 2,
    url: 'https://two.example/feed.xml',
    title: 'Two',
    format: 'atom',
    etag: null,
    lastModified: null,
    intervalSecs: 1800,
    nextFetchAt: 0,
    lastError: null,
    consecutiveFailures: 0,
    createdAt: 0,
    unreadCount: 0,
    folderId: null,
    hiddenFromAll: false,
  },
];

describe('FeedList', () => {
  it('shows aggregate and per-feed unread counts', () => {
    render(FeedList, {
      props: {
        feeds,
        folders: [],
        selectedFeedId: null,
        selectedFolderId: null,
        onSelectFeed: vi.fn(),
        onSelectFolder: vi.fn(),
        onManageFolders: vi.fn(),
        onMoveFeed: vi.fn(),
      },
    });

    expect(screen.getByRole('button', { name: /All\s*2/ })).toBeTruthy();
    expect(screen.getByRole('button', { name: /One\s*2/ })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Two' })).toBeTruthy();
  });

  it('selects the requested feed and marks its row selected', async () => {
    const onSelectFeed = vi.fn();
    render(FeedList, {
      props: {
        feeds,
        folders: [],
        selectedFeedId: 1,
        selectedFolderId: null,
        onSelectFeed,
        onSelectFolder: vi.fn(),
        onManageFolders: vi.fn(),
        onMoveFeed: vi.fn(),
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Two' }));
    expect(onSelectFeed).toHaveBeenCalledWith(2);
    expect(screen.getByRole('button', { name: /One/ }).classList.contains('selected')).toBe(true);
  });
});
