import type { AiContextItem, Item, ItemFilter } from '$lib/types';

/** Explicit contexts pin immediately; an untouched single-article draft follows the reader. */
export class ConversationContext {
  items = $state<AiContextItem[]>([]);
  scopeLabel = $state<string | null>(null);
  pendingScopeFilter = $state<ItemFilter | null>(null);
  pinned = $state(false);

  reset(
    items: Item[] = [],
    scope: string | null = null,
    pinned = false,
    filter: ItemFilter | null = null,
  ) {
    this.items = items.map((item, position) => ({
      itemId: item.id,
      itemTitle: item.title,
      feedId: item.feedId,
      publishedAt: item.publishedAt,
      position,
    }));
    this.scopeLabel = scope;
    this.pendingScopeFilter = filter;
    this.pinned = pinned;
  }
  get ids() {
    return this.items.flatMap((i) => (i.itemId === null ? [] : [i.itemId]));
  }
  contains(id: number) {
    return this.items.some((i) => i.itemId === id);
  }
  add(items: Item[]) {
    this.pendingScopeFilter = null;
    for (const item of items) {
      if (this.contains(item.id) || this.items.length >= 20) continue;
      this.items.push({
        itemId: item.id,
        itemTitle: item.title,
        feedId: item.feedId,
        publishedAt: item.publishedAt,
        position: Math.max(-1, ...this.items.map((i) => i.position)) + 1,
      });
    }
    this.pinned = true;
  }
}
