import { fireEvent, render, within } from '@testing-library/svelte';
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
  },
];

const items: Item[] = [
  {
    id: 10,
    feedId: 4,
    guidHash: 'newer',
    title: 'Newest first',
    link: null,
    summary: null,
    fullContent: null,
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
    summary: null,
    fullContent: null,
    publishedAt: 100,
    fetchedAt: 100,
    isRead: true,
    readAt: 150,
    isSaved: false,
  },
];

describe('PostsFeed', () => {
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
});
