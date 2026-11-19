import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { tick } from 'svelte';
import { listen } from '@tauri-apps/api/event';
import type { FeedSummary, Item, ItemFilter } from '$lib/types';
import type { ItemCursor } from '$lib/api';

const api = vi.hoisted(() => ({
  listFeeds: vi.fn(),
  listFeedFolders: vi.fn(),
  listItems: vi.fn(),
  getItemSummary: vi.fn(),
  countUnreadItems: vi.fn(),
  listAiSessions: vi.fn(),
  markRead: vi.fn(),
  markItemsRead: vi.fn(),
  markItemsUnread: vi.fn(),
  refreshFeed: vi.fn(),
  refreshFeedBatch: vi.fn(),
  setItemSaved: vi.fn(),
  updateFeedOrganization: vi.fn(),
  getFeedIcons: vi.fn(),
  getResourceSettings: vi.fn(),
  setResourceSettings: vi.fn(),
  setResourceVisibility: vi.fn(),
}));
vi.mock('$lib/api', () => api);
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
const navigation = vi.hoisted(() => ({
  before: null as null | ((nav: { to: { url: URL } }) => void),
}));
vi.mock('$app/navigation', () => ({
  goto: vi.fn(),
  beforeNavigate: (callback: typeof navigation.before) => {
    navigation.before = callback;
  },
}));
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));
vi.mock('$lib/components/PostReader.svelte', async () => ({
  default: (await import('./__fixtures__/ReaderProbe.svelte')).default,
}));
import Page from '../../routes/+page.svelte';
import { managedCache } from '$lib/cacheBudget';
import { takeReadingCheckpoint } from '$lib/readingCheckpoint';

let data: Item[];
function matching(filter: ItemFilter) {
  return data.filter(
    (item) =>
      (!filter.unreadOnly || !item.isRead) &&
      (!filter.savedOnly || item.isSaved) &&
      (!filter.query || item.title?.toLowerCase().includes(filter.query.toLowerCase())) &&
      (!filter.feedId || item.feedId === filter.feedId) &&
      (filter.publishedFrom == null || item.publishedAt >= filter.publishedFrom) &&
      (filter.publishedTo == null || item.publishedAt < filter.publishedTo),
  );
}

beforeEach(() => {
  localStorage.clear();
  takeReadingCheckpoint();
  vi.clearAllMocks();
  api.getItemSummary.mockImplementation(
    async (id: number) => data.find((item) => item.id === id) ?? null,
  );
  const noon = new Date();
  noon.setHours(12, 0, 0, 0);
  data = Array.from({ length: 330 }, (_, index) => ({
    id: 330 - index,
    feedId: 1,
    guidHash: String(index),
    title: `Post ${330 - index}`,
    link: null,
    preview: null,
    publishedAt: Math.floor(noon.getTime() / 1000) + 330 - index,
    fetchedAt: 1,
    isRead: false,
    readAt: null,
    isSaved: false,
  }));
  const feed: FeedSummary = {
    id: 1,
    url: 'https://test.invalid/rss',
    title: 'Feed',
    format: 'rss',
    etag: null,
    lastModified: null,
    intervalSecs: 900,
    nextFetchAt: 0,
    lastError: null,
    consecutiveFailures: 0,
    createdAt: 1,
    unreadCount: 330,
    folderId: null,
    hiddenFromAll: false,
    language: null,
    customTitle: null,
    sortOrder: 0,
    pinned: false,
  };
  api.listFeeds.mockImplementation(async () => [
    { ...feed, unreadCount: data.filter((item) => !item.isRead).length },
  ]);
  api.listFeedFolders.mockResolvedValue([]);
  api.listAiSessions.mockResolvedValue([]);
  api.countUnreadItems.mockImplementation(
    async (filter: ItemFilter) => matching(filter).filter((item) => !item.isRead).length,
  );
  api.markRead.mockImplementation(async (id: number) => {
    data.find((item) => item.id === id)!.isRead = true;
  });
  api.setItemSaved.mockImplementation(async (id: number, saved: boolean) => {
    data.find((item) => item.id === id)!.isSaved = saved;
  });
  api.listItems.mockImplementation(
    async (filter: ItemFilter, limit: number, cursor: ItemCursor | null, direction: string) => {
      const rows = matching(filter).filter(
        (item) =>
          !cursor ||
          (direction === 'newer'
            ? (item.publishedAt - cursor.publishedAt || item.id - cursor.id) > 0
            : (item.publishedAt - cursor.publishedAt || item.id - cursor.id) < 0),
      );
      return (direction === 'newer' ? rows.slice(-limit) : rows.slice(0, limit)).map((item) => ({
        ...item,
      }));
    },
  );
});

describe('page/window integration', () => {
  it('re-queries a saved search and selected post after settings without retaining old page bytes', async () => {
    const first = render(Page);
    await screen.findByText('Post 330');
    const list = first.container.querySelector('.posts-scroll') as HTMLElement;
    await fireEvent.click(within(list).getByText('Post 330'));
    await fireEvent.click(within(list).getByRole('button', { name: 'Search posts' }));
    await fireEvent.input(within(list).getByLabelText('Search posts'), {
      target: { value: 'Post 3' },
    });
    await waitFor(() =>
      expect(api.listItems).toHaveBeenLastCalledWith(
        expect.objectContaining({ query: 'Post 3' }),
        50,
        null,
        'older',
      ),
    );
    navigation.before?.({
      to: { url: new URL('https://aequo.local/settings?section=performance') },
    });
    first.unmount();
    expect(managedCache.bytes).toBe(0);
    api.listItems.mockClear();
    const second = render(Page);
    await waitFor(() => expect(screen.getByTestId('reader-probe').dataset.itemId).toBe('330'));
    expect(api.getItemSummary).toHaveBeenCalledWith(330);
    expect(api.listItems).toHaveBeenCalledWith(
      expect.objectContaining({ query: 'Post 3' }),
      50,
      expect.any(Object),
      'newer',
    );
    const returned = second.container.querySelector('.posts-scroll') as HTMLElement;
    expect(within(returned).getByLabelText('Search posts')).toHaveProperty('value', 'Post 3');
    expect(second.container.querySelectorAll('.post').length).toBeLessThanOrEqual(100);
    second.unmount();
    expect(managedCache.bytes).toBe(0);
  });
  it('handles a selected post deleted in settings by closing the reader', async () => {
    const first = render(Page);
    await screen.findByText('Post 330');
    const list = first.container.querySelector('.posts-scroll') as HTMLElement;
    await fireEvent.click(within(list).getByText('Post 330'));
    navigation.before?.({ to: { url: new URL('https://aequo.local/settings') } });
    first.unmount();
    data = data.filter((item) => item.id !== 330);
    const second = render(Page);
    await waitFor(() => expect(api.getItemSummary).toHaveBeenCalledWith(330));
    await waitFor(() =>
      expect(second.container.querySelector('[data-post-id="329"]')).toBeTruthy(),
    );
    expect(screen.queryByTestId('reader-probe')).toBeNull();
    second.unmount();
  });

  it('does not insert a retained reader post into a starred search that excludes it', async () => {
    for (const item of data) item.isSaved = true;
    const { container } = render(Page);
    await screen.findByText('Post 330');
    await fireEvent.click(screen.getByRole('button', { name: 'Starred' }));
    const list = container.querySelector('.posts-scroll') as HTMLElement;
    await waitFor(() => expect(within(list).getByText('Post 330')).toBeTruthy());
    await fireEvent.click(within(list).getByText('Post 330'));
    await fireEvent.click(within(list).getByRole('button', { name: 'Search posts' }));
    await fireEvent.input(within(list).getByLabelText('Search posts'), {
      target: { value: 'Post 1' },
    });
    await waitFor(() => expect(within(list).queryByText('Post 330')).toBeNull());
    await fireEvent.click(screen.getByRole('button', { name: 'Toggle retained save' }));
    await waitFor(() => expect(api.setItemSaved).toHaveBeenLastCalledWith(330, false));
    await fireEvent.click(screen.getByRole('button', { name: 'Toggle retained save' }));
    await waitFor(() =>
      expect(api.listItems).toHaveBeenLastCalledWith(
        expect.objectContaining({ savedOnly: true, query: 'Post 1' }),
        1,
        expect.objectContaining({ id: 331 }),
      ),
    );
    expect(within(list).queryByText('Post 330')).toBeNull();
    expect(screen.getByTestId('reader-probe').dataset.itemId).toBe('330');
  });
  it('keeps the open post and whole-view unread count while pages are evicted and restored', async () => {
    const { container, unmount } = render(Page);
    await screen.findByText('Post 330');
    await fireEvent.click(screen.getByRole('button', { name: 'Today' }));
    await waitFor(() => expect(api.countUnreadItems).toHaveBeenCalled());
    const list = container.querySelector('.posts-scroll') as HTMLElement;
    await fireEvent.click(within(list).getByText('Post 330'));
    await waitFor(() => expect(screen.getByTestId('reader-probe').dataset.itemId).toBe('330'));
    await fireEvent.click(screen.getByRole('button', { name: 'Today' }));
    expect(screen.getByTestId('reader-probe').dataset.itemId).toBe('330');
    await waitFor(() =>
      expect(within(list).getByRole('radio', { name: /Unread/ }).textContent).toContain('329'),
    );
    for (let index = 0; index < 6; index++) {
      const button = within(list).getByRole('button', { name: 'Load older posts' });
      await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
      await fireEvent.click(button);
      await tick();
      await waitFor(() => expect(api.listItems).toHaveBeenCalledTimes(index + 3));
    }
    await waitFor(() =>
      expect(within(list).queryByRole('button', { name: 'Load older posts' })).toBeNull(),
    );
    expect(container.querySelector('[data-post-id="330"]')).toBeNull();
    expect(screen.getByTestId('reader-probe').dataset.itemId).toBe('330');
    expect(within(list).getByRole('radio', { name: /Unread/ }).textContent).toContain('329');
    expect(container.querySelectorAll('.post').length).toBeLessThanOrEqual(100);
    for (let index = 0; index < 6; index++) {
      const button = within(list).queryByRole('button', { name: 'Load newer posts' });
      if (!button) break;
      await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
      await fireEvent.click(button);
      await tick();
      await waitFor(() =>
        expect(
          container.querySelector('[data-post-id="330"]') ||
            !(button as HTMLButtonElement).disabled ||
            !button.isConnected,
        ).toBeTruthy(),
      );
    }
    await waitFor(() => expect(container.querySelector('[data-post-id="330"]')).toBeTruthy());
    expect(screen.getByTestId('reader-probe').dataset.itemId).toBe('330');
    expect(api.countUnreadItems.mock.calls.length).toBeLessThan(5);
    unmount();
    expect(managedCache.bytes).toBe(0);
  });
});

it('replaces the current page after metadata repair even when no new articles arrived', async () => {
  render(Page);
  await screen.findByText('Post 330');
  const callback = vi.mocked(listen).mock.calls.find(([name]) => name === 'feed-updated')?.[1];
  expect(callback).toBeDefined();
  data = data.slice(1);
  callback!({
    event: 'feed-updated',
    id: 1,
    payload: { feedId: 1, newItemCount: 0, reconciled: true },
  });
  await waitFor(() => expect(screen.queryByText('Post 330')).toBeNull());
  expect(screen.getByText('Post 329')).toBeTruthy();
});
