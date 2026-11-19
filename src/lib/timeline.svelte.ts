import type { Item, ItemFilter } from './types';
import type { ItemCursor } from './api';
import { CacheBudget, managedCache, serializedBytes } from './cacheBudget';

type FetchPage = (
  filter: ItemFilter,
  limit: number,
  cursor?: ItemCursor | null,
  direction?: 'older' | 'newer',
) => Promise<Item[]>;
let nextOwner = 0;

function sortedUnique(items: Item[]): Item[] {
  return [...new Map(items.map((item) => [item.id, item])).values()].sort(
    (a, b) => b.publishedAt - a.publishedAt || b.id - a.id,
  );
}

/** Bounded, bidirectional list state. Stale responses cannot cross a view change. */
export class TimelineWindow {
  items = $state<Item[]>([]);
  hasOlder = $state(false);
  hasNewer = $state(false);
  loading = $state(false);
  loadingPage = $state(false);
  error = $state<string | null>(null);
  resetToken = $state(0);
  anchorId: number | null = null;
  private filter: ItemFilter = {};
  private filterKey = '';
  private generation = 0;
  private disposed = false;
  private owner = `timeline:${++nextOwner}`;
  private maximumItems = 100;

  constructor(
    private fetchPage: FetchPage,
    private budget: CacheBudget = managedCache,
  ) {}

  invalidate(): void {
    this.generation++;
    this.loading = false;
    this.loadingPage = false;
  }

  setPageLimit(pages: number): void {
    this.maximumItems = Math.max(2, Math.min(5, pages)) * 50;
    this.trim('anchor');
  }

  private trim(side: 'older' | 'newer' | 'anchor', maximumBytes = Number.MAX_SAFE_INTEGER): number {
    const original = this.items;
    let start = 0;
    let end = original.length;
    if (end > this.maximumItems) {
      if (side === 'older') start = end - this.maximumItems;
      else if (side === 'newer') end = this.maximumItems;
      else {
        const anchor = Math.max(
          0,
          original.findIndex((item) => item.id === this.anchorId),
        );
        start = Math.min(
          Math.max(0, anchor - Math.floor(this.maximumItems / 2)),
          end - this.maximumItems,
        );
        end = start + this.maximumItems;
      }
    }
    let kept = original.slice(start, end);
    // A very large legacy title/URL can hit the byte budget before the page cap.
    // Remove from the edge farthest from the current visible anchor.
    let bytes = kept.length ? serializedBytes(kept) : 0;
    if (bytes > maximumBytes) this.error = 'cacheBudget';
    while (bytes > maximumBytes && kept.length) {
      const anchor = kept.findIndex((item) => item.id === this.anchorId);
      if (anchor >= kept.length / 2) {
        start++;
        kept = kept.slice(1);
      } else {
        end--;
        kept = kept.slice(0, -1);
      }
      bytes = kept.length ? serializedBytes(kept) : 0;
    }
    if (start > 0) this.hasNewer = true;
    if (end < original.length) this.hasOlder = true;
    if (start > 0 || end < original.length) this.items = kept;
    if (!kept.length && original.length) this.error = 'cacheBudget';
    return bytes;
  }

  /** Also called after optimistic metadata edits so their bytes stay accounted. */
  account(): void {
    if (this.disposed) return;
    const bytes = this.items.length ? serializedBytes(this.items) : 0;
    this.budget.hold(this.owner, bytes, (maximum) => this.trim('anchor', maximum));
  }

  private accept(items: Item[], side: 'older' | 'newer' | 'anchor') {
    this.items = sortedUnique(items);
    this.trim(side);
    this.account();
  }

  async refresh(filter: ItemFilter, background = false): Promise<boolean> {
    if (this.disposed) return false;
    const key = JSON.stringify(filter);
    const sameView = key === this.filterKey;
    // New arrivals must not teleport a reader who has scrolled into history.
    if (background && sameView && this.hasNewer) return false;
    const generation = ++this.generation;
    this.filter = { ...filter };
    this.filterKey = key;
    this.loading = true;
    this.loadingPage = false;
    this.error = null;
    if (!sameView) {
      this.items = [];
      this.hasNewer = false;
      this.hasOlder = false;
      this.anchorId = null;
      this.account();
    }
    try {
      const page = await this.fetchPage({ ...filter }, 50, null, 'older');
      if (generation !== this.generation || this.disposed) return false;
      if (background && sameView && this.items.length) {
        const head = page[0];
        const current = this.items[0];
        // A burst can insert more than a page above the reader. Merging only
        // the latest 50 with the old window would silently leave a gap. Keep
        // the current window; the newer cursor walks every intervening page.
        if (
          head &&
          (head.publishedAt > current.publishedAt ||
            (head.publishedAt === current.publishedAt && head.id > current.id))
        )
          this.hasNewer = true;
        return false;
      } else {
        this.hasNewer = false;
        this.hasOlder = page.length === 50;
        this.anchorId = null;
        this.accept(page, 'newer');
        this.resetToken++;
      }
      return true;
    } catch (error) {
      if (generation === this.generation && !this.disposed) this.error = String(error);
      return false;
    } finally {
      if (generation === this.generation && !this.disposed) this.loading = false;
    }
  }

  /** Re-query a bounded window around a route handoff, never retain old pages. */
  async resume(filter: ItemFilter, cursor: ItemCursor): Promise<boolean> {
    if (this.disposed) return false;
    const generation = ++this.generation;
    this.filter = { ...filter };
    this.filterKey = JSON.stringify(filter);
    this.loading = true;
    this.error = null;
    this.anchorId = cursor.id;
    this.loadingPage = false;
    try {
      const [newer, older] = await Promise.all([
        this.fetchPage({ ...filter }, 50, cursor, 'newer'),
        // IDs are integer SQLite keys; include the anchor in the older side.
        this.fetchPage(
          { ...filter },
          50,
          { publishedAt: cursor.publishedAt, id: cursor.id + 1 },
          'older',
        ),
      ]);
      if (this.disposed || generation !== this.generation) return false;
      this.hasNewer = newer.length === 50;
      this.hasOlder = older.length === 50;
      this.accept([...newer, ...older], 'anchor');
      this.resetToken++;
      return true;
    } catch (error) {
      if (!this.disposed && generation === this.generation) this.error = String(error);
      return false;
    } finally {
      if (!this.disposed && generation === this.generation) this.loading = false;
    }
  }

  async more(direction: 'older' | 'newer'): Promise<void> {
    if (this.disposed || this.loading || this.loadingPage || !this.items.length) return;
    if (direction === 'older' ? !this.hasOlder : !this.hasNewer) return;
    const generation = this.generation;
    const cursor = direction === 'older' ? this.items.at(-1)! : this.items[0];
    this.loadingPage = true;
    this.error = null;
    try {
      const page = await this.fetchPage(
        { ...this.filter },
        50,
        { publishedAt: cursor.publishedAt, id: cursor.id },
        direction,
      );
      if (generation !== this.generation || this.disposed) return;
      if (direction === 'older') this.hasOlder = page.length === 50;
      else this.hasNewer = page.length === 50;
      this.accept([...this.items, ...page], direction);
    } catch (error) {
      if (generation === this.generation && !this.disposed) this.error = String(error);
    } finally {
      if (generation === this.generation && !this.disposed) this.loadingPage = false;
    }
  }

  remove(id: number): void {
    this.items = this.items.filter((item) => item.id !== id);
    this.account();
  }

  retainFeeds(valid: Set<number>): void {
    const kept = this.items.filter((item) => valid.has(item.feedId));
    if (kept.length === this.items.length) return;
    this.items = kept;
    if (!kept.length) {
      this.hasOlder = false;
      this.hasNewer = false;
    }
    this.account();
  }

  restore(item: Item): void {
    this.accept([...this.items, item], 'anchor');
  }

  dispose(): void {
    this.disposed = true;
    this.generation++;
    this.items = [];
    this.budget.release(this.owner);
  }
}
