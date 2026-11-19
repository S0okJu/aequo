import { describe, expect, it } from 'vitest';
import { ConversationContext } from './context.svelte';
import type { Item } from '$lib/types';
const item = (id: number) => ({ id, title: `Article ${id}`, feedId: 1 }) as Item;
describe('explicit conversation contexts', () => {
  it('follows an untouched article but pins explicit additions and preserves their order', () => {
    const context = new ConversationContext();
    context.reset([item(1)]);
    expect(context.pinned).toBe(false);
    context.add([item(2), item(1)]);
    expect(context.pinned).toBe(true);
    expect(context.ids).toEqual([1, 2]);
    context.reset([item(3)], 'Today', true);
    expect(context.ids).toEqual([3]);
    expect(context.scopeLabel).toBe('Today');
    expect(context.pinned).toBe(true);
  });
  it('caps context at 20 distinct articles and retains deleted-source metadata', () => {
    const context = new ConversationContext();
    context.add(Array.from({ length: 30 }, (_, i) => item(i + 1)));
    expect(context.ids).toHaveLength(20);
    context.items[0].itemId = null;
    expect(context.ids).toHaveLength(19);
    expect(context.items[0].itemTitle).toBe('Article 1');
  });
});
