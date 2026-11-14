<script lang="ts">
  import type { Item, FeedSummary } from '$lib/types';

  interface Props {
    items: Item[];
    feeds: FeedSummary[];
    selectedItemId: number | null;
    onSelect: (itemId: number) => void;
  }

  let { items, feeds, selectedItemId, onSelect }: Props = $props();

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
  {#each items as item (item.id)}
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
  {/each}
  {#if items.length === 0}
    <p class="empty">No items.</p>
  {/if}
</div>

<style>
  .timeline {
    height: 100%;
    overflow-y: auto;
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    border: none;
    border-bottom: 1px solid var(--color-border);
    background: transparent;
    text-align: left;
    cursor: default;
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
