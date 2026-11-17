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
    language: null,
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
    language: null,
  },
];

describe('FeedList', () => {
  const baseProps = {
    folders: [],
    selectedFolderId: null,
    onSelectFeed: vi.fn(),
    onSelectFolder: vi.fn(),
    onManageFolders: vi.fn(),
    onMoveFeed: vi.fn(),
  };

  it('hides feeds with nothing unread in unread-only mode, except the selected one', async () => {
    const { rerender } = render(FeedList, {
      props: { ...baseProps, feeds, selectedFeedId: null, unreadOnly: true },
    });
    expect(screen.getByRole('button', { name: /One\s*2/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Two' })).toBeNull();

    await rerender({ ...baseProps, feeds, selectedFeedId: 2, unreadOnly: true });
    expect(screen.getByRole('button', { name: 'Two' })).toBeTruthy();
  });

  it('says so when every feed is read in unread-only mode', () => {
    render(FeedList, {
      props: {
        ...baseProps,
        feeds: feeds.map((feed) => ({ ...feed, unreadCount: 0 })),
        selectedFeedId: null,
        unreadOnly: true,
      },
    });
    expect(screen.getByText('All caught up')).toBeTruthy();
  });

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
