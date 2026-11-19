import { fireEvent, render, screen, within } from '@testing-library/svelte';
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
    customTitle: null,
    sortOrder: 0,
    pinned: false,
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
    customTitle: null,
    sortOrder: 0,
    pinned: false,
  },
];

describe('FeedList', () => {
  const baseProps = {
    folders: [],
    selectedFolderId: null,
    onSelectFeed: vi.fn(),
    onSelectFolder: vi.fn(),
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
        onMoveFeed: vi.fn(),
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Two' }));
    expect(onSelectFeed).toHaveBeenCalledWith(2);
    expect(screen.getByRole('button', { name: /^One/ }).classList.contains('selected')).toBe(true);
  });

  it('offers the same actions from the row menu and from right-click', async () => {
    const onMarkRead = vi.fn();
    const onOpenSettings = vi.fn();
    render(FeedList, {
      props: {
        ...baseProps,
        feeds,
        selectedFeedId: null,
        onMarkRead,
        onOpenSettings,
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Actions for One' }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Mark as read' }));
    expect(onMarkRead).toHaveBeenCalledWith({ feedId: 1 });
    expect(screen.queryByRole('menu')).toBeNull();

    await fireEvent.contextMenu(screen.getByRole('button', { name: /^One/ }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Feed settings…' }));
    expect(onOpenSettings).toHaveBeenCalledWith({ feedId: 1 });
  });

  function fakeDataTransfer(): DataTransfer {
    const store: Record<string, string> = {};
    return {
      effectAllowed: '',
      setData: (type: string, value: string) => {
        store[type] = value;
      },
      getData: (type: string) => store[type] ?? '',
    } as unknown as DataTransfer;
  }

  it('filters feeds and folders by name, expanding a matching folder', async () => {
    const { rerender } = render(FeedList, {
      props: {
        ...baseProps,
        feeds: [{ ...feeds[0], folderId: 9 }, feeds[1]],
        folders: [{ id: 9, name: 'Tech', sortOrder: 0, createdAt: 0 }],
        selectedFeedId: null,
      },
    });
    await rerender({
      ...baseProps,
      feeds: [{ ...feeds[0], folderId: 9 }, feeds[1]],
      folders: [{ id: 9, name: 'Tech', sortOrder: 0, createdAt: 0 }],
      selectedFeedId: null,
      filter: 'one',
    });
    expect(screen.getByRole('button', { name: /^One/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Two' })).toBeNull();

    await rerender({
      ...baseProps,
      feeds: [{ ...feeds[0], folderId: 9 }, feeds[1]],
      folders: [{ id: 9, name: 'Tech', sortOrder: 0, createdAt: 0 }],
      selectedFeedId: null,
      filter: 'nope',
    });
    expect(screen.getByText('No feeds match your filter.')).toBeTruthy();
  });

  it('pins and unpins a feed from the row menu, and lists pinned feeds up top', async () => {
    const onTogglePinned = vi.fn();
    const { rerender } = render(FeedList, {
      props: { ...baseProps, feeds, selectedFeedId: null, onTogglePinned },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Actions for One' }));
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Pin' }));
    expect(onTogglePinned).toHaveBeenCalledWith(1, true);

    await rerender({
      ...baseProps,
      feeds: feeds.map((feed) => (feed.id === 1 ? { ...feed, pinned: true } : feed)),
      selectedFeedId: null,
      onTogglePinned,
    });
    const pinnedGroup = screen.getByRole('group', { name: 'Pinned' });
    await fireEvent.click(within(pinnedGroup).getByRole('button', { name: 'Actions for One' }));
    expect(screen.getByRole('menuitem', { name: 'Unpin' })).toBeTruthy();
  });

  it('reorders feeds within the unfiled group on drop', async () => {
    const onReorderFeeds = vi.fn();
    const three = { ...feeds[1], id: 3, url: 'https://three.example/feed.xml', title: 'Three' };
    render(FeedList, {
      props: { ...baseProps, feeds: [...feeds, three], selectedFeedId: null, onReorderFeeds },
    });

    const dataTransfer = fakeDataTransfer();
    const oneRow = screen.getByRole('button', { name: /^One/ });
    const threeRow = screen.getByRole('button', { name: 'Three' }).closest('.item-row');
    if (!threeRow) throw new Error('row not found');
    await fireEvent.dragStart(oneRow, { dataTransfer });
    await fireEvent.dragOver(threeRow, { dataTransfer });
    await fireEvent.drop(threeRow, { dataTransfer });

    expect(onReorderFeeds).toHaveBeenCalledWith(null, [2, 1, 3]);
  });

  it('remembers collapsed folders across mounts', async () => {
    localStorage.removeItem('aequo:collapsedFolders');
    const props = {
      ...baseProps,
      feeds: [{ ...feeds[0], folderId: 9 }],
      folders: [{ id: 9, name: 'Tech', sortOrder: 0, createdAt: 0 }],
      selectedFeedId: null,
    };
    const first = render(FeedList, { props });
    await fireEvent.click(screen.getByRole('button', { name: 'Collapse Tech' }));
    expect(screen.queryByRole('button', { name: /^One/ })).toBeNull();
    first.unmount();

    render(FeedList, { props });
    expect(screen.getByRole('button', { name: 'Expand Tech' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /^One/ })).toBeNull();
  });
});
