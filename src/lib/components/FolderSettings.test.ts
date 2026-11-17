import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { createFeedFolder, renameFeedFolder, deleteFeedFolder } = vi.hoisted(() => ({
  createFeedFolder: vi.fn(),
  renameFeedFolder: vi.fn(),
  deleteFeedFolder: vi.fn(),
}));
vi.mock('$lib/api', () => ({ createFeedFolder, renameFeedFolder, deleteFeedFolder }));

import FolderSettings from './FolderSettings.svelte';
import type { FeedFolder, FeedSummary } from '$lib/types';

function sampleFolder(overrides: Partial<FeedFolder> = {}): FeedFolder {
  return { id: 1, name: 'Tech', sortOrder: 0, createdAt: 0, ...overrides };
}

function sampleFeed(overrides: Partial<FeedSummary> = {}): FeedSummary {
  return {
    id: 1,
    url: 'https://example.com/feed.xml',
    title: 'Example',
    format: 'rss',
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
    ...overrides,
  };
}

describe('FolderSettings', () => {
  beforeEach(() => {
    createFeedFolder.mockReset();
    renameFeedFolder.mockReset();
    deleteFeedFolder.mockReset();
  });

  it('shows an empty state when there are no folders', () => {
    render(FolderSettings, { props: { feeds: [], folders: [], onFoldersChanged: vi.fn() } });
    expect(screen.getByText('No folders yet')).toBeTruthy();
  });

  it('shows the feed count badge per folder', () => {
    const folders = [sampleFolder({ id: 1, name: 'Tech' }), sampleFolder({ id: 2, name: 'News' })];
    const feeds = [
      sampleFeed({ id: 1, folderId: 1 }),
      sampleFeed({ id: 2, folderId: 1 }),
      sampleFeed({ id: 3, folderId: 2 }),
    ];
    render(FolderSettings, { props: { feeds, folders, onFoldersChanged: vi.fn() } });

    expect(screen.getByText('2 feeds')).toBeTruthy();
    expect(screen.getByText('1 feed')).toBeTruthy();
  });

  it('creates a folder and reports the change', async () => {
    createFeedFolder.mockResolvedValue(1);
    const onFoldersChanged = vi.fn();
    render(FolderSettings, { props: { feeds: [], folders: [], onFoldersChanged } });

    await fireEvent.click(screen.getByText('+ New folder'));
    const input = screen.getByPlaceholderText('Folder name');
    await fireEvent.input(input, { target: { value: 'Science' } });
    await fireEvent.click(screen.getByText('Create'));

    await waitFor(() => expect(createFeedFolder).toHaveBeenCalledWith('Science'));
    expect(onFoldersChanged).toHaveBeenCalled();
  });

  it('rejects creating a folder with a name that already exists', async () => {
    const folders = [sampleFolder({ name: 'Tech' })];
    render(FolderSettings, { props: { feeds: [], folders, onFoldersChanged: vi.fn() } });

    await fireEvent.click(screen.getByText('+ New folder'));
    await fireEvent.input(screen.getByPlaceholderText('Folder name'), {
      target: { value: 'Tech' },
    });
    await fireEvent.click(screen.getByText('Create'));

    expect(await screen.findByText(/already exists/)).toBeTruthy();
    expect(createFeedFolder).not.toHaveBeenCalled();
  });

  it('renames a folder', async () => {
    renameFeedFolder.mockResolvedValue(undefined);
    const folders = [sampleFolder({ id: 5, name: 'Tech' })];
    const onFoldersChanged = vi.fn();
    render(FolderSettings, { props: { feeds: [], folders, onFoldersChanged } });

    await fireEvent.click(screen.getByText('Rename'));
    const input = screen.getByDisplayValue('Tech');
    await fireEvent.input(input, { target: { value: 'Technology' } });
    await fireEvent.click(screen.getByText('Save'));

    await waitFor(() => expect(renameFeedFolder).toHaveBeenCalledWith(5, 'Technology'));
    expect(onFoldersChanged).toHaveBeenCalled();
  });

  it('deletes a folder only after confirming', async () => {
    deleteFeedFolder.mockResolvedValue(undefined);
    const folders = [sampleFolder({ id: 5, name: 'Tech' })];
    const onFoldersChanged = vi.fn();
    render(FolderSettings, { props: { feeds: [], folders, onFoldersChanged } });

    await fireEvent.click(screen.getByText('Delete'));
    expect(deleteFeedFolder).not.toHaveBeenCalled();
    expect(screen.getByText(/Delete "Tech"\?/)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(deleteFeedFolder).toHaveBeenCalledWith(5));
    expect(onFoldersChanged).toHaveBeenCalled();
  });
});
