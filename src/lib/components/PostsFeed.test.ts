import { fireEvent, render, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { describe, expect, it, vi } from 'vitest';

import PostsFeed from './PostsFeed.svelte';
import type { FeedSummary, Item } from '$lib/types';

const feeds: FeedSummary[] = [
  {
    id: 4,
    url: 'https://daily.example/rss.xml',
    title: 'Daily',
    format: 'rss',
    etag: null,
    lastModified: null,
    intervalSecs: 1800,
    nextFetchAt: 0,
    lastError: null,
    consecutiveFailures: 0,
    createdAt: 0,
    unreadCount: 1,
    folderId: null,
    hiddenFromAll: false,
    language: null,
    customTitle: null,
    sortOrder: 0,
    pinned: false,
  },
];

const items: Item[] = [
  {
    id: 10,
    feedId: 4,
    guidHash: 'newer',
    title: 'Newest first',
    link: null,
    preview: null,
    publishedAt: 200,
    fetchedAt: 200,
    isRead: false,
    readAt: null,
    isSaved: false,
  },
  {
    id: 9,
    feedId: 4,
    guidHash: 'older',
    title: 'Older second',
    link: null,
    preview: null,
    publishedAt: 100,
    fetchedAt: 100,
    isRead: true,
    readAt: 150,
    isSaved: false,
  },
];

describe('PostsFeed', () => {
  it('restores a variable-height visible row after a page is removed above it', async () => {
    const original = HTMLElement.prototype.getBoundingClientRect;
    const rectangle = (top: number, height: number) =>
      ({
        top,
        bottom: top + height,
        height,
        left: 0,
        right: 280,
        width: 280,
        x: 0,
        y: top,
      }) as DOMRect;
    const rowHeight = (node: HTMLElement) =>
      node.dataset.rowId
        ? 100 + (Number(node.dataset.rowId) % 3) * 20
        : Number.parseFloat(node.style.height) || 0;
    const geometry = vi
      .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
      .mockImplementation(function (this: HTMLElement) {
        const scroll = this.closest('.posts-scroll') as HTMLElement | null;
        if (!scroll) return original.call(this);
        if (this.classList.contains('posts-scroll')) return rectangle(0, 600);
        if (this.classList.contains('feed-context')) return rectangle(0, 48);
        const rows = scroll.querySelector('.rows') as HTMLElement | null;
        if (!rows) return original.call(this);
        const newer = [...scroll.querySelectorAll('.page-button')].some(
          (button) => button.textContent?.trim() === 'Load newer posts',
        );
        const base = 48 + (newer ? 48 : 0) - scroll.scrollTop;
        if (this === rows)
          return rectangle(
            base,
            [...rows.children].reduce((sum, child) => sum + rowHeight(child as HTMLElement), 0),
          );
        if (this.dataset.rowId) {
          let top = base;
          for (const child of rows.children) {
            if (child === this) break;
            top += rowHeight(child as HTMLElement);
          }
          return rectangle(top, rowHeight(this));
        }
        return original.call(this);
      });
    try {
      const initial = Array.from({ length: 100 }, (_, index) => ({
        ...items[0],
        id: 200 - index,
        title: `Post ${200 - index}`,
      }));
      const { container, rerender, unmount } = render(PostsFeed, {
        props: {
          items: initial,
          feeds,
          folders: [],
          selectedFeedId: null,
          selectedFolderId: null,
          selectedItemId: null,
          hasMore: false,
          onSelect: vi.fn(),
          onToggleSaved: vi.fn(),
          onEndReached: vi.fn(),
        },
      });
      const scroll = container.querySelector('.posts-scroll') as HTMLElement;
      const anchor = () => {
        const row = [...container.querySelectorAll<HTMLElement>('[data-row-id]')].find(
          (node) =>
            node.getBoundingClientRect().bottom > 48 && node.getBoundingClientRect().top < 600,
        );
        return row ? { id: row.dataset.rowId, offset: row.getBoundingClientRect().top - 48 } : null;
      };
      scroll.scrollTop = 9000;
      await fireEvent.scroll(scroll);
      await tick();
      await tick();
      const before = anchor();
      expect(before).not.toBeNull();
      expect(Number(before!.id)).toBeLessThan(151);
      const more = Array.from({ length: 50 }, (_, index) => ({
        ...items[0],
        id: 100 - index,
        title: `Post ${100 - index}`,
      }));
      await rerender({ items: [...initial.slice(50), ...more], hasNewer: true });
      await waitFor(() => {
        expect(anchor()?.id).toBe(before!.id);
        expect(Math.abs(anchor()!.offset - before!.offset)).toBeLessThan(1);
      });
      expect(container.querySelectorAll('.post').length).toBeLessThanOrEqual(100);
      const checkpoint = anchor()!;
      unmount();
      const resumed = render(PostsFeed, {
        props: {
          items: [...initial.slice(50), ...more],
          feeds,
          folders: [],
          selectedFeedId: null,
          selectedFolderId: null,
          selectedItemId: null,
          hasMore: false,
          hasNewer: true,
          resumePosition: { id: Number(checkpoint.id), offset: checkpoint.offset },
          onSelect: vi.fn(),
          onToggleSaved: vi.fn(),
          onEndReached: vi.fn(),
        },
      });
      await waitFor(() => {
        const row = resumed.container.querySelector<HTMLElement>(
          `[data-row-id="${checkpoint.id}"]`,
        );
        expect(row).toBeTruthy();
        expect(Math.abs(row!.getBoundingClientRect().top - 48 - checkpoint.offset)).toBeLessThan(1);
        expect((document.activeElement as HTMLElement).dataset.postId).toBe(checkpoint.id);
      });
    } finally {
      geometry.mockRestore();
    }
  });
  it('mounts at most 100 rows for 10,000 inputs and preserves offscreen keyboard focus', async () => {
    const many = Array.from({ length: 10000 }, (_, index) => ({
      ...items[0],
      id: 10000 - index,
      title: `Post ${10000 - index}`,
    }));
    const { container } = render(PostsFeed, {
      props: {
        items: many,
        feeds,
        folders: [],
        selectedFeedId: null,
        selectedFolderId: null,
        selectedItemId: null,
        hasMore: false,
        onSelect: vi.fn(),
        onToggleSaved: vi.fn(),
        onEndReached: vi.fn(),
      },
    });
    const scroll = container.querySelector('.posts-scroll') as HTMLDivElement;
    const rows = container.querySelector('.rows') as HTMLDivElement;
    const first = container.querySelector('.post') as HTMLElement;
    first.focus();
    await tick();
    scroll.getBoundingClientRect = () => ({ top: 0, bottom: 600, height: 600 }) as DOMRect;
    rows.getBoundingClientRect = () =>
      ({
        top: 48 - scroll.scrollTop,
        bottom: 1400000 - scroll.scrollTop,
        height: 1400000,
      }) as DOMRect;
    Object.defineProperty(scroll, 'clientHeight', { value: 600 });
    Object.defineProperty(scroll, 'scrollHeight', { value: 1400000 });
    scroll.scrollTop = 700000;
    await fireEvent.scroll(scroll);
    await tick();
    expect(container.querySelectorAll('.post').length).toBeLessThanOrEqual(100);
    expect(container.querySelector('[data-post-id="5000"]')).toBeTruthy();
    expect(first.isConnected).toBe(true);
    expect(document.activeElement).toBe(first);
    await fireEvent.keyDown(first, { key: 'ArrowDown' });
    await waitFor(() =>
      expect((document.activeElement as HTMLElement).dataset.postId).toBe('9999'),
    );
  });
  it('keeps keyboard activation of a save button separate from opening the post', async () => {
    const onSelect = vi.fn();
    const onToggleSaved = vi.fn();
    const { container } = render(PostsFeed, {
      props: {
        items: [items[0]],
        feeds,
        folders: [],
        selectedFeedId: null,
        selectedFolderId: null,
        selectedItemId: null,
        hasMore: false,
        onSelect,
        onToggleSaved,
        onEndReached: vi.fn(),
      },
    });
    const post = container.querySelector('.post') as HTMLElement;
    const save = within(post).getByRole('button', { name: 'Save post' });
    await fireEvent.keyDown(save, { key: 'Enter' });
    await fireEvent.keyDown(save, { key: ' ' });
    await fireEvent.click(save);
    expect(onSelect).not.toHaveBeenCalled();
    expect(onToggleSaved).toHaveBeenCalledWith(10);

    await fireEvent.keyDown(post, { key: 'Enter' });
    await fireEvent.keyDown(post, { key: ' ' });
    expect(onSelect).toHaveBeenCalledTimes(2);
    expect(onSelect).toHaveBeenLastCalledWith(10);
  });

  it('keeps feed icons visible and opens posts with ordinary or modifier clicks', async () => {
    const onSelect = vi.fn();
    const { container, queryByRole } = render(PostsFeed, {
      props: {
        items,
        feeds,
        folders: [],
        selectedFeedId: null,
        selectedFolderId: null,
        selectedItemId: null,
        hasMore: false,
        onSelect,
        onToggleSaved: vi.fn(),
        onEndReached: vi.fn(),
      },
    });
    expect(queryByRole('checkbox')).toBeNull();
    const post = container.querySelector('.post') as HTMLElement;
    expect(post.querySelector('.feed-icon')).toBeTruthy();
    await fireEvent.click(post, { metaKey: true });
    await fireEvent.click(post, { ctrlKey: true });
    await fireEvent.click(post, { shiftKey: true });
    expect(onSelect).toHaveBeenCalledTimes(3);
    expect(onSelect).toHaveBeenLastCalledWith(10);
    expect(container.querySelector('.selection-bar')).toBeNull();
  });

  it('renders the provided newest-first order and marks unread posts', () => {
    const { container } = render(PostsFeed, {
      props: {
        items,
        feeds,
        folders: [],
        selectedFeedId: null,
        selectedFolderId: null,
        selectedItemId: null,
        hasMore: false,
        onSelect: vi.fn(),
        onToggleSaved: vi.fn(),
        onEndReached: vi.fn(),
      },
    });

    const posts = container.querySelectorAll('.post');
    expect(within(posts[0] as HTMLElement).getByText('Newest first')).toBeTruthy();
    expect(posts[0].classList.contains('unread')).toBe(true);
    expect(within(posts[0] as HTMLElement).getByText('Unread')).toBeTruthy();
    expect(within(posts[1] as HTMLElement).getByText('Older second')).toBeTruthy();
    expect(posts[1].classList.contains('unread')).toBe(false);
    expect(within(posts[1] as HTMLElement).queryByText('Unread')).toBeNull();
  });

  it('shortens tagged feed names and shows a full timestamp on hover', () => {
    const { container } = render(PostsFeed, {
      props: {
        items: [items[0]],
        feeds: [{ ...feeds[0], title: 'Daily | News, Notes, and More' }],
        folders: [],
        selectedFeedId: null,
        selectedFolderId: null,
        selectedItemId: null,
        hasMore: false,
        onSelect: vi.fn(),
        onToggleSaved: vi.fn(),
        onEndReached: vi.fn(),
      },
    });

    const post = container.querySelector('.post') as HTMLElement;
    expect(within(post).getByText('Daily')).toBeTruthy();
    expect(post.querySelector('time')?.getAttribute('title')).toBeTruthy();
  });

  it('switches between all/unread and marks the view read from the toolbar', async () => {
    const onUnreadOnlyChange = vi.fn();
    const onMarkAllRead = vi.fn();
    const { getByRole } = render(PostsFeed, {
      props: {
        items,
        feeds,
        folders: [],
        selectedFeedId: null,
        selectedFolderId: null,
        selectedItemId: null,
        hasMore: false,
        unreadCount: 5,
        onSelect: vi.fn(),
        onToggleSaved: vi.fn(),
        onEndReached: vi.fn(),
        onUnreadOnlyChange,
        onMarkAllRead,
      },
    });

    expect(getByRole('radio', { name: 'All' }).getAttribute('aria-checked')).toBe('true');
    await fireEvent.click(getByRole('radio', { name: 'Unread' }));
    expect(onUnreadOnlyChange).toHaveBeenCalledWith(true);

    await fireEvent.click(getByRole('button', { name: 'Mark all as read' }));
    expect(onMarkAllRead).toHaveBeenCalledOnce();
  });

  it('searches the view and highlights matches in titles', async () => {
    const onQueryChange = vi.fn();
    const props = {
      items,
      feeds,
      folders: [],
      selectedFeedId: null,
      selectedFolderId: null,
      selectedItemId: null,
      hasMore: false,
      onSelect: vi.fn(),
      onToggleSaved: vi.fn(),
      onEndReached: vi.fn(),
      onQueryChange,
    };
    const { getByRole, getByLabelText, container, rerender } = render(PostsFeed, { props });

    await fireEvent.click(getByRole('button', { name: 'Search posts' }));
    await fireEvent.input(getByLabelText('Search posts'), { target: { value: 'newest' } });
    expect(onQueryChange).toHaveBeenLastCalledWith('newest');

    await rerender({ ...props, query: 'newest' });
    expect(container.querySelector('.title mark')?.textContent).toBe('Newest');

    await fireEvent.keyDown(getByLabelText('Search posts'), { key: 'Escape' });
    expect(onQueryChange).toHaveBeenLastCalledWith('');
  });
});
