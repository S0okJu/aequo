import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

import {
  createAnnotation,
  deleteAnnotation,
  listItemAnnotations,
  listItems,
  markItemsRead,
  markItemsUnread,
  markRead,
  updateAnnotation,
} from './api';

describe('Tauri API wrappers', () => {
  beforeEach(() => invoke.mockReset());

  it('forwards the view filter and paging to the list_items command', async () => {
    invoke.mockResolvedValue([]);
    await listItems({ unreadOnly: true, feedId: 7 }, 50, { publishedAt: 1_700_000_000, id: 100 });

    expect(invoke).toHaveBeenCalledWith('list_items', {
      filter: { unreadOnly: true, feedId: 7 },
      limit: 50,
      cursorPublishedAt: 1_700_000_000,
      cursorId: 100,
    });
  });

  it('forwards a null cursor for the first page', async () => {
    invoke.mockResolvedValue([]);
    await listItems({ savedOnly: true }, 50);

    expect(invoke).toHaveBeenCalledWith('list_items', {
      filter: { savedOnly: true },
      limit: 50,
      cursorPublishedAt: null,
      cursorId: null,
    });
  });

  it('forwards the view filter and load time to mark_items_read, and ids to undo', async () => {
    invoke.mockResolvedValue([1, 2]);
    await expect(markItemsRead({ folderId: 3 }, 1_700_000_000)).resolves.toEqual([1, 2]);
    expect(invoke).toHaveBeenCalledWith('mark_items_read', {
      filter: { folderId: 3 },
      fetchedBefore: 1_700_000_000,
    });

    invoke.mockResolvedValue(2);
    await markItemsUnread([1, 2]);
    expect(invoke).toHaveBeenCalledWith('mark_items_unread', { itemIds: [1, 2] });
  });

  it('forwards the selected item to mark_read', async () => {
    invoke.mockResolvedValue(undefined);
    await markRead(23);

    expect(invoke).toHaveBeenCalledWith('mark_read', { itemId: 23 });
  });

  it('forwards the item id to list_item_annotations', async () => {
    invoke.mockResolvedValue([]);
    await listItemAnnotations(42);

    expect(invoke).toHaveBeenCalledWith('list_item_annotations', { itemId: 42 });
  });

  it('forwards highlight fields to create_annotation', async () => {
    invoke.mockResolvedValue({});
    await createAnnotation({
      itemId: 42,
      kind: 'highlight',
      contentHash: 'hash-1',
      startOffset: 0,
      endOffset: 5,
      quotedText: 'hello',
      color: 'yellow',
    });

    expect(invoke).toHaveBeenCalledWith('create_annotation', {
      itemId: 42,
      kind: 'highlight',
      contentHash: 'hash-1',
      startOffset: 0,
      endOffset: 5,
      quotedText: 'hello',
      color: 'yellow',
      note: null,
    });
  });

  it('forwards memo fields to update_annotation', async () => {
    invoke.mockResolvedValue({});
    await updateAnnotation({
      id: 7,
      kind: 'memo',
      contentHash: 'hash-2',
      startOffset: 10,
      endOffset: 20,
      quotedText: 'world',
      note: 'a note',
    });

    expect(invoke).toHaveBeenCalledWith('update_annotation', {
      id: 7,
      kind: 'memo',
      contentHash: 'hash-2',
      startOffset: 10,
      endOffset: 20,
      quotedText: 'world',
      color: null,
      note: 'a note',
    });
  });

  it('forwards the annotation id to delete_annotation', async () => {
    invoke.mockResolvedValue(undefined);
    await deleteAnnotation(7);

    expect(invoke).toHaveBeenCalledWith('delete_annotation', { id: 7 });
  });
});
