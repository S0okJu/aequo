<script lang="ts">
  import type { Item, FeedSummary } from '$lib/types';
  import VirtualList from './VirtualList.svelte';

  interface Props {
    items: Item[];
    feeds: FeedSummary[];
    selectedItemId: number | null;
    onSelect: (itemId: number) => void;
    onEndReached?: () => void;
  }

  let { items, feeds, selectedItemId, onSelect, onEndReached }: Props = $props();

  // Must match .row's fixed height exactly (padding + two text lines)
  // - VirtualList's index math depends on every row being this tall.
  const ROW_HEIGHT = 56;

  function feedName(feedId: number): string {
    const feed = feeds.find((f) => f.id === feedId);
    return feed?.title ?? feed?.url ?? 'Unknown feed';
  }

  function formatTime(unixSecs: number): string {
    return new Date(unixSecs * 1000).toLocaleString(undefined, {
      dateStyle: 'short',
      timeStyle: 'short',
    });
  }
</script>

<div class="timeline">
  {#if items.length === 0}
    <p class="empty">No items.</p>
  {:else}
    <VirtualList {items} itemHeight={ROW_HEIGHT} {onEndReached}>
      {#snippet children(item)}
        <button
          class="row"
          class:selected={selectedItemId === item.id}
          class:unread={!item.isRead}
          onclick={() => onSelect(item.id)}
        >
          <span class="indicator" aria-hidden="true"></span>
          <span class="content">
            <span class="title">{item.title ?? '(untitled)'}</span>
            <span class="meta">{feedName(item.feedId)} · {formatTime(item.publishedAt)}</span>
          </span>
        </button>
      {/snippet}
    </VirtualList>
  {/if}
</div>

<style>
  .timeline {
    height: 100%;
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    width: 100%;
    height: 56px;
    padding: 8px 12px;
    border: none;
    border-bottom: 1px solid var(--color-border);
    background: transparent;
    text-align: left;
    cursor: default;
    box-sizing: border-box;
  }

  .row:hover {
    background: var(--color-hover-bg);
  }

  .row.selected {
    background: var(--color-selection-bg);
  }

  .indicator {
    flex-shrink: 0;
    width: 6px;
    height: 6px;
    margin-top: 6px;
    border-radius: 50%;
    background: var(--color-accent);
    visibility: hidden;
  }

  .row.unread .indicator {
    visibility: visible;
  }

  .content {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title {
    font-size: 0.95em;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row.unread .title {
    font-weight: 600;
  }

  .row:not(.unread) .title {
    color: var(--color-text-secondary);
  }

  .meta {
    font-size: 0.8em;
    color: var(--color-text-tertiary);
    margin-top: 2px;
  }

  .empty {
    padding: 12px;
    color: var(--color-text-tertiary);
    font-size: 0.85em;
  }
</style>
