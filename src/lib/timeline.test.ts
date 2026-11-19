import { describe, expect, it } from 'vitest';
import { CacheBudget } from './cacheBudget';
import { TimelineWindow } from './timeline.svelte';
import type { Item, ItemFilter } from './types';
import type { ItemCursor } from './api';

const data: Item[] = Array.from({ length: 10000 }, (_, index) => {
  const id = 10000 - index;
  return {
    id,
    feedId: (id % 2) + 1,
    guidHash: String(id),
    title: `Post ${id}`,
    link: null,
    preview: '한글 preview',
    publishedAt: Math.floor(id / 4),
    fetchedAt: 1,
    isRead: false,
    readAt: null,
    isSaved: false,
  };
});

async function fetchPage(
  filter: ItemFilter,
  limit: number,
  cursor: ItemCursor | null = null,
  direction = 'older',
) {
  const matching = data
    .filter((item) => !filter.feedId || item.feedId === filter.feedId)
    .filter((item) => {
      if (!cursor) return true;
      const comparison = item.publishedAt - cursor.publishedAt || item.id - cursor.id;
      return direction === 'newer' ? comparison > 0 : comparison < 0;
    });
  return direction === 'newer' ? matching.slice(-limit) : matching.slice(0, limit);
}

describe('bounded timeline', () => {
  it('does not skip a burst larger than one page when refreshing the visible head', async () => {
    let source = data;
    const window = new TimelineWindow(async (_filter, limit, cursor, direction) => {
      const matching = source.filter(
        (item) =>
          !cursor ||
          (direction === 'newer'
            ? (item.publishedAt - cursor.publishedAt || item.id - cursor.id) > 0
            : (item.publishedAt - cursor.publishedAt || item.id - cursor.id) < 0),
      );
      return direction === 'newer' ? matching.slice(-limit) : matching.slice(0, limit);
    }, new CacheBudget(1000000));
    await window.refresh({});
    const old = window.items.map((item) => item.id);
    const newRows = Array.from({ length: 1000 }, (_, index) => ({
      ...data[0],
      id: 11000 - index,
      publishedAt: 100000 - index,
    }));
    source = [...newRows, ...data];
    expect(await window.refresh({}, true)).toBe(false);
    expect(window.items.map((item) => item.id)).toEqual(old);
    const seen = new Set<number>();
    while (window.hasNewer) {
      await window.more('newer');
      for (const item of window.items) if (item.id > 10000) seen.add(item.id);
    }
    expect(seen.size).toBe(1000);
    expect(window.items[0].id).toBe(11000);
  });
  it('traverses 10,000 posts in both directions with timestamp ties and no retained growth', async () => {
    const cache = new CacheBudget(16 * 1024 * 1024);
    const window = new TimelineWindow(fetchPage, cache);
    window.setPageLimit(2);
    await window.refresh({});
    const seen = new Set(window.items.map((item) => item.id));
    let requests = 0;
    while (window.hasOlder) {
      window.anchorId = window.items.at(-5)?.id ?? null;
      await window.more('older');
      for (const item of window.items) seen.add(item.id);
      expect(window.items.length).toBeLessThanOrEqual(100);
      expect(new Set(window.items.map((item) => item.id)).size).toBe(window.items.length);
      expect(cache.bytes).toBeLessThanOrEqual(cache.limit);
      if (++requests > 210) throw new Error('older cursor did not advance');
    }
    expect(seen.size).toBe(10000);
    expect(window.items.at(-1)?.id).toBe(1);
    requests = 0;
    const returned = new Set(window.items.map((item) => item.id));
    while (window.hasNewer) {
      window.anchorId = window.items[4]?.id ?? null;
      await window.more('newer');
      for (const item of window.items) returned.add(item.id);
      expect(window.items.length).toBeLessThanOrEqual(100);
      if (++requests > 210) throw new Error('newer cursor did not advance');
    }
    expect(returned.size).toBe(10000);
    expect(window.items[0].id).toBe(10000);
    window.dispose();
    expect(cache.bytes).toBe(0);
  });

  it('shrinks page and byte budgets around the visible anchor and keeps both cursors usable', async () => {
    const cache = new CacheBudget(1000000);
    const window = new TimelineWindow(fetchPage, cache);
    window.setPageLimit(5);
    await window.refresh({});
    for (let index = 0; index < 4; index++) await window.more('older');
    expect(window.items.length).toBe(250);
    window.anchorId = window.items[170].id;
    const anchor = window.anchorId;
    window.setPageLimit(2);
    window.account();
    expect(window.items.length).toBe(100);
    expect(window.items.some((item) => item.id === anchor)).toBe(true);
    expect(window.hasNewer && window.hasOlder).toBe(true);
    cache.setLimit(1500);
    expect(cache.bytes).toBeLessThanOrEqual(1500);
    expect(window.items.some((item) => item.id === anchor)).toBe(true);
    expect(window.error).toBe('cacheBudget');
  });

  it('ignores in-flight old pages after a filter change and does not jump history on refresh', async () => {
    let finish: (page: Item[]) => void = () => {};
    let paused = false;
    const window = new TimelineWindow((filter, limit, cursor, direction) => {
      if (paused && cursor)
        return new Promise((resolve) => {
          finish = resolve;
        });
      return fetchPage(filter, limit, cursor, direction);
    }, new CacheBudget(1000000));
    await window.refresh({});
    paused = true;
    const pending = window.more('older');
    await window.refresh({ feedId: 2 });
    finish(data.slice(50, 100));
    await pending;
    expect(window.items.every((item) => item.feedId === 2)).toBe(true);
    expect(window.loadingPage).toBe(false);
    paused = false;
    await window.more('older');
    await window.more('older');
    const ids = window.items.map((item) => item.id);
    expect(window.hasNewer).toBe(true);
    expect(await window.refresh({ feedId: 2 }, true)).toBe(false);
    expect(window.items.map((item) => item.id)).toEqual(ids);
  });
  it('resumes a middle cursor with timestamp ties, then continues both directions without gaps', async () => {
    const cache = new CacheBudget(1000000);
    const window = new TimelineWindow(fetchPage, cache);
    window.setPageLimit(2);
    const anchor = data.find((item) => item.id === 5000)!;
    expect(await window.resume({}, { id: anchor.id, publishedAt: anchor.publishedAt })).toBe(true);
    expect(window.items).toHaveLength(100);
    expect(window.items.map((item) => item.id)).toEqual(
      data.slice(4950, 5050).map((item) => item.id),
    );
    expect(window.anchorId).toBe(5000);
    expect(window.hasOlder && window.hasNewer).toBe(true);
    await window.more('older');
    expect(window.items.map((item) => item.id)).toEqual(
      data.slice(5000, 5100).map((item) => item.id),
    );
    await window.more('newer');
    expect(window.items.map((item) => item.id)).toEqual(
      data.slice(4950, 5050).map((item) => item.id),
    );
    window.dispose();
    expect(cache.bytes).toBe(0);
  });
  it('does not restore stale route data into a filter the reader has since changed', async () => {
    let finish!: (items: Item[]) => void;
    const delayed = new Promise<Item[]>((resolve) => {
      finish = resolve;
    });
    const window = new TimelineWindow((filter, limit, cursor, direction) => {
      if (!filter.feedId) return delayed;
      return fetchPage(filter, limit, cursor, direction);
    }, new CacheBudget(1000000));
    const pending = window.resume({}, { id: 5000, publishedAt: 1250 });
    await window.refresh({ feedId: 2 });
    finish(data.slice(4950, 5050));
    expect(await pending).toBe(false);
    expect(window.items.every((item) => item.feedId === 2)).toBe(true);
    window.dispose();
  });
});
