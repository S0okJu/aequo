<script lang="ts">
  import type { FeedSummary } from '$lib/types';

  interface Props {
    feeds: FeedSummary[];
    selectedFeedId: number | null;
    onSelect: (feedId: number | null) => void;
  }

  let { feeds, selectedFeedId, onSelect }: Props = $props();

  const totalUnread = $derived(feeds.reduce((sum, f) => sum + f.unreadCount, 0));
</script>

<nav class="feed-list">
  <button class="row" class:selected={selectedFeedId === null} onclick={() => onSelect(null)}>
    <span class="title">All</span>
    {#if totalUnread > 0}
      <span class="count">{totalUnread}</span>
    {/if}
  </button>

  {#each feeds as feed (feed.id)}
    <button
      class="row"
      class:selected={selectedFeedId === feed.id}
      onclick={() => onSelect(feed.id)}
    >
      <span class="title">{feed.title ?? feed.url}</span>
      {#if feed.unreadCount > 0}
        <span class="count">{feed.unreadCount}</span>
      {/if}
    </button>
  {/each}
</nav>

<style>
  .feed-list {
    height: 100%;
    overflow-y: auto;
    padding: 8px 4px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 6px 10px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--color-text);
    font-size: 0.9em;
    text-align: left;
    cursor: default;
  }

  .row:hover {
    background: var(--color-hover-bg);
  }

  .row.selected {
    background: var(--color-selection-bg);
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    flex-shrink: 0;
    margin-left: 8px;
    color: var(--color-text-secondary);
    font-size: 0.85em;
  }
</style>
