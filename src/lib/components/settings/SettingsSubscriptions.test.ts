import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { FeedFolder, FeedSummary } from '$lib/types';

const api = vi.hoisted(() => ({
  createFeedFolder: vi.fn(),
  deleteFeed: vi.fn(),
  deleteFeedFolder: vi.fn(),
  getFeedRawContent: vi.fn(),
  refreshFeed: vi.fn(),
  renameFeed: vi.fn(),
  renameFeedFolder: vi.fn(),
  setFeedRefreshInterval: vi.fn(),
  updateFeedOrganization: vi.fn(),
}));
vi.mock('$lib/api', () => api);

import SettingsSubscriptions from './SettingsSubscriptions.svelte';

function feed(id: number, title: string, overrides: Partial<FeedSummary> = {}): FeedSummary {
  return {
    id,
    url: `https://${title.toLowerCase().replace(/\s+/g, '')}.example/feed.xml`,
    title,
    format: 'rss',
    etag: null,
    lastModified: null,
    intervalSecs: 900,
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
    ...overrides,
  };
}

const folders: FeedFolder[] = [{ id: 1, name: 'Tech', sortOrder: 0, createdAt: 0 }];
const feeds = [
  feed(10, 'Alpha Blog', { folderId: 1 }),
  feed(11, 'Beta News'),
  feed(12, 'Broken Feed', { lastError: 'HTTP 503' }),
];

function renderList(props: Partial<{ feeds: FeedSummary[]; folders: FeedFolder[] }> = {}) {
  const onChanged = vi.fn();
  render(SettingsSubscriptions, {
    props: { feeds, folders, onChanged, onAddFeed: vi.fn(), ...props },
  });
  return { onChanged };
}

describe('SettingsSubscriptions', () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset().mockResolvedValue(undefined);
  });

  it('filters by name or URL and counts results', async () => {
    renderList();
    expect(screen.getByText('3 feeds')).toBeTruthy();
    await fireEvent.input(screen.getByLabelText('Search subscriptions'), {
      target: { value: 'beta' },
    });
    expect(screen.getByText('1 result')).toBeTruthy();
    expect(screen.getByRole('button', { name: /Beta News/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Alpha Blog/ })).toBeNull();
  });

  it('lists failing feeds under "Needs attention" as well as in their group', () => {
    renderList();
    const attention = screen.getByRole('region', { name: 'Needs attention' });
    expect(within(attention).getByRole('button', { name: /Broken Feed/ })).toBeTruthy();
  });

  it('opens details beside the list and saves changes immediately', async () => {
    const { onChanged } = renderList();
    await fireEvent.click(screen.getByRole('button', { name: /Alpha Blog/ }));
    const detail = screen.getByRole('complementary');
    expect(within(detail).getByRole('heading', { name: 'Alpha Blog' })).toBeTruthy();

    await fireEvent.change(within(detail).getByLabelText('Check for new posts'), {
      target: { value: '3600' },
    });
    expect(api.setFeedRefreshInterval).toHaveBeenCalledWith(10, 3600);

    await fireEvent.click(within(detail).getByRole('switch', { name: /Show in All/ }));
    expect(api.updateFeedOrganization).toHaveBeenCalledWith(10, 1, true);
    await vi.waitFor(() => expect(onChanged).toHaveBeenCalled());

    await fireEvent.click(within(detail).getByRole('button', { name: 'Close details' }));
    expect(screen.queryByRole('complementary')).toBeNull();
  });

  it('saves a display name and clears it back to the feed title', async () => {
    renderList();
    await fireEvent.click(screen.getByRole('button', { name: /Beta News/ }));
    const input = screen.getByLabelText('Display name');
    await fireEvent.input(input, { target: { value: '  Beta  ' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(api.renameFeed).toHaveBeenCalledWith(11, 'Beta');
  });

  it('only unsubscribes after confirming', async () => {
    renderList();
    await fireEvent.click(screen.getByRole('button', { name: /Beta News/ }));
    await fireEvent.click(screen.getByRole('button', { name: 'Unsubscribe' }));
    expect(api.deleteFeed).not.toHaveBeenCalled();
    const confirm = screen.getByText(/Unsubscribe from "Beta News"/).closest('.confirm-row')!;
    await fireEvent.click(
      within(confirm as HTMLElement).getByRole('button', { name: 'Unsubscribe' }),
    );
    expect(api.deleteFeed).toHaveBeenCalledWith(11);
  });

  it("shows the feed's last-fetched raw content in a dialog", async () => {
    api.getFeedRawContent.mockResolvedValue({
      content: '<rss><channel><title>Beta News</title></channel></rss>',
      fetchedAt: 1_700_000_000,
    });
    renderList();
    await fireEvent.click(screen.getByRole('button', { name: /Beta News/ }));
    await fireEvent.click(screen.getByRole('button', { name: 'View source' }));
    expect(api.getFeedRawContent).toHaveBeenCalledWith(11);

    const dialog = await screen.findByRole('heading', { name: /Source · Beta News/ });
    expect(
      within(dialog.closest('dialog')!).getByText(
        '<rss><channel><title>Beta News</title></channel></rss>',
      ),
    ).toBeTruthy();
  });

  it('shows an empty state when a feed has no stored source yet', async () => {
    api.getFeedRawContent.mockResolvedValue(null);
    renderList();
    await fireEvent.click(screen.getByRole('button', { name: /Beta News/ }));
    await fireEvent.click(screen.getByRole('button', { name: 'View source' }));
    expect(await screen.findByText(/No source is stored yet/)).toBeTruthy();
  });

  it('creates folders, rejecting duplicate names', async () => {
    renderList();
    await fireEvent.click(screen.getByRole('button', { name: 'New folder' }));
    const name = screen.getByLabelText('Folder name');
    await fireEvent.input(name, { target: { value: 'tech' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(screen.getByRole('alert').textContent).toContain('already exists');
    expect(api.createFeedFolder).not.toHaveBeenCalled();

    await fireEvent.input(name, { target: { value: 'Design' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(api.createFeedFolder).toHaveBeenCalledWith('Design');
  });

  it('offers a retry for an unexpected rename failure, unlike a name clash', async () => {
    renderList();
    api.renameFeedFolder.mockRejectedValueOnce(new Error('disk full'));
    await fireEvent.click(screen.getByRole('button', { name: 'Rename Tech' }));
    await fireEvent.input(screen.getByLabelText('Folder name'), {
      target: { value: 'Engineering' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('disk full');
    const retry = within(alert as HTMLElement).getByRole('button', { name: 'Retry' });

    api.renameFeedFolder.mockResolvedValueOnce(undefined);
    await fireEvent.click(retry);
    expect(api.renameFeedFolder).toHaveBeenCalledTimes(2);
    expect(api.renameFeedFolder).toHaveBeenLastCalledWith(1, 'Engineering');
  });

  it('renames a folder and deletes one only after confirming', async () => {
    renderList();
    await fireEvent.click(screen.getByRole('button', { name: 'Rename Tech' }));
    await fireEvent.input(screen.getByLabelText('Folder name'), {
      target: { value: 'Engineering' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(api.renameFeedFolder).toHaveBeenCalledWith(1, 'Engineering');

    // The rename settles asynchronously before the folder header returns.
    await fireEvent.click(await screen.findByRole('button', { name: 'Delete Tech' }));
    expect(api.deleteFeedFolder).not.toHaveBeenCalled();
    expect(screen.getByText(/Its 1 feed moves to Unfiled/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(api.deleteFeedFolder).toHaveBeenCalledWith(1);
  });
});
