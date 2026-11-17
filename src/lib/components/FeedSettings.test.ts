import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const {
  deleteFeed,
  refreshFeed,
  getRetentionSettings,
  setRetentionSettings,
  previewRetentionCleanup,
  runRetentionCleanup,
  updateFeedOrganization,
  setFeedRefreshInterval,
  addFeed,
  createFeedFolder,
  renameFeedFolder,
  deleteFeedFolder,
} = vi.hoisted(() => ({
  deleteFeed: vi.fn(),
  refreshFeed: vi.fn(),
  getRetentionSettings: vi.fn(),
  setRetentionSettings: vi.fn(),
  previewRetentionCleanup: vi.fn(),
  runRetentionCleanup: vi.fn(),
  updateFeedOrganization: vi.fn(),
  setFeedRefreshInterval: vi.fn(),
  addFeed: vi.fn(),
  createFeedFolder: vi.fn(),
  renameFeedFolder: vi.fn(),
  deleteFeedFolder: vi.fn(),
}));
vi.mock('$lib/api', () => ({
  deleteFeed,
  refreshFeed,
  getRetentionSettings,
  setRetentionSettings,
  previewRetentionCleanup,
  runRetentionCleanup,
  updateFeedOrganization,
  setFeedRefreshInterval,
  addFeed,
  createFeedFolder,
  renameFeedFolder,
  deleteFeedFolder,
}));

import FeedSettings from './FeedSettings.svelte';
import type { FeedSummary } from '$lib/types';

function sampleFeed(overrides: Partial<FeedSummary> = {}): FeedSummary {
  return {
    id: 1,
    url: 'https://example.com/feed.xml',
    title: 'Example Feed',
    format: 'rss',
    etag: null,
    lastModified: null,
    intervalSecs: 1800,
    nextFetchAt: 0,
    lastError: null,
    consecutiveFailures: 0,
    createdAt: 0,
    unreadCount: 3,
    folderId: null,
    hiddenFromAll: false,
    ...overrides,
  };
}

describe('FeedSettings', () => {
  beforeEach(() => {
    deleteFeed.mockReset();
    refreshFeed.mockReset();
    updateFeedOrganization.mockReset();
    setFeedRefreshInterval.mockReset();
    getRetentionSettings
      .mockReset()
      .mockResolvedValue({ readRetentionDays: 365, fullTextRetentionDays: 180 });
    previewRetentionCleanup
      .mockReset()
      .mockResolvedValue({ articlesToRemove: 0, fullTextsToClear: 0, estimatedBytesToFree: 0 });
    runRetentionCleanup.mockReset();
  });

  it('defaults to the Feeds tab and shows the subscriptions list', async () => {
    render(FeedSettings, {
      props: {
        feeds: [sampleFeed()],
        folders: [],
        onAdded: vi.fn(),
        onChanged: vi.fn(),
        onBack: vi.fn(),
      },
    });

    expect(screen.getByRole('tab', { name: 'Feeds' }).getAttribute('aria-selected')).toBe('true');
    expect(screen.getByText('Example Feed')).toBeTruthy();
    expect(screen.getByText('3 unread')).toBeTruthy();
  });

  it('opens directly on the Folders tab when initialTab is "folders"', () => {
    render(FeedSettings, {
      props: {
        feeds: [],
        folders: [],
        onAdded: vi.fn(),
        onChanged: vi.fn(),
        onBack: vi.fn(),
        initialTab: 'folders',
      },
    });

    expect(screen.getByRole('tab', { name: 'Folders' }).getAttribute('aria-selected')).toBe('true');
    expect(screen.getByText('No folders yet')).toBeTruthy();
  });

  it('switches tabs on click, showing only the active panel', async () => {
    render(FeedSettings, {
      props: {
        feeds: [sampleFeed()],
        folders: [],
        onAdded: vi.fn(),
        onChanged: vi.fn(),
        onBack: vi.fn(),
      },
    });

    await fireEvent.click(screen.getByRole('tab', { name: 'Storage' }));
    expect(screen.getByRole('tab', { name: 'Storage' }).getAttribute('aria-selected')).toBe('true');
    expect(screen.getByText('Storage & retention')).toBeTruthy();
    expect(screen.queryByText('Example Feed')).toBeNull();
  });

  it('requires a confirm step before unsubscribing from a feed', async () => {
    deleteFeed.mockResolvedValue(undefined);
    const onChanged = vi.fn();
    render(FeedSettings, {
      props: {
        feeds: [sampleFeed({ title: 'Example Feed' })],
        folders: [],
        onAdded: vi.fn(),
        onChanged,
        onBack: vi.fn(),
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Unsubscribe' }));
    expect(deleteFeed).not.toHaveBeenCalled();
    expect(screen.getByText(/Unsubscribe from "Example Feed"\?/)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Unsubscribe' }));
    await waitFor(() => expect(deleteFeed).toHaveBeenCalledWith(1));
    expect(onChanged).toHaveBeenCalled();
  });

  it('saves a feed-specific refresh interval', async () => {
    setFeedRefreshInterval.mockResolvedValue(undefined);
    const onChanged = vi.fn();
    render(FeedSettings, { props: { feeds: [sampleFeed()], folders: [], onAdded: vi.fn(), onChanged, onBack: vi.fn() } });
    await fireEvent.change(screen.getByRole('combobox', { name: 'Refresh interval for Example Feed' }), { target: { value: '3600' } });
    await waitFor(() => expect(setFeedRefreshInterval).toHaveBeenCalledWith(1, 3600));
    expect(onChanged).toHaveBeenCalled();
  });

  it('uses a consistently labeled switch to include a feed in All', async () => {
    updateFeedOrganization.mockResolvedValue(undefined);
    const onChanged = vi.fn();
    render(FeedSettings, {
      props: {
        feeds: [sampleFeed({ hiddenFromAll: true })],
        folders: [],
        onAdded: vi.fn(),
        onChanged,
        onBack: vi.fn(),
      },
    });

    const showInAll = screen.getByRole('switch', { name: 'Show Example Feed in All' });
    expect((showInAll as HTMLInputElement).checked).toBe(false);
    await fireEvent.click(showInAll);

    await waitFor(() => expect(updateFeedOrganization).toHaveBeenCalledWith(1, null, false));
    expect(onChanged).toHaveBeenCalled();
  });
});
