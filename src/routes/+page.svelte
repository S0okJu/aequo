<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import FeedList from '$lib/components/FeedList.svelte';
  import Timeline from '$lib/components/Timeline.svelte';
  import DetailView from '$lib/components/DetailView.svelte';
  import { listFeeds, listItems } from '$lib/api';
  import type { FeedSummary, Item } from '$lib/types';

  const PAGE_SIZE = 50;

  interface FeedUpdatedPayload {
    feedId: number;
    newItemCount: number;
  }

  let feeds = $state<FeedSummary[]>([]);
  let items = $state<Item[]>([]);
  let hasMoreItems = $state(true);
  let loadingMoreItems = false; // plain guard against overlapping loads, not UI state
  let selectedFeedId = $state<number | null>(null);
  let selectedItemId = $state<number | null>(null);

  async function refreshFeeds() {
    feeds = await listFeeds();
  }

  // First page of the merged timeline (§6.3) for the current feed
  // selection - replaces items entirely (a feed-updated event or a
  // feed-selection change can shift what "page 1" even means).
  async function refreshItems() {
    items = await listItems(PAGE_SIZE, 0, false, selectedFeedId);
    hasMoreItems = items.length === PAGE_SIZE;
  }

  // Infinite scroll: VirtualList calls this when the user nears the
  // bottom (onEndReached). Appends the next page via limit/offset.
  async function loadMoreItems() {
    if (loadingMoreItems || !hasMoreItems) return;
    loadingMoreItems = true;
    try {
      const nextPage = await listItems(PAGE_SIZE, items.length, false, selectedFeedId);
      items = [...items, ...nextPage];
      hasMoreItems = nextPage.length === PAGE_SIZE;
    } finally {
      loadingMoreItems = false;
    }
  }

  function selectFeed(feedId: number | null) {
    selectedFeedId = feedId;
    refreshItems();
  }

  onMount(() => {
    refreshFeeds();
    refreshItems();

    // Event-driven only (§3.2/§6.2) - no polling timer anywhere here.
    const unlisten = listen<FeedUpdatedPayload>('feed-updated', () => {
      refreshFeeds();
      refreshItems();
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  });
</script>

<div class="layout">
  <aside class="pane feed-list-pane">
    <FeedList {feeds} {selectedFeedId} onSelect={selectFeed} />
  </aside>
  <main class="pane timeline-pane">
    <Timeline
      {items}
      {feeds}
      {selectedItemId}
      onSelect={(itemId) => (selectedItemId = itemId)}
      onEndReached={loadMoreItems}
    />
  </main>
  <aside class="pane detail-pane">
    <DetailView />
  </aside>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 200px 1fr 1fr;
    height: 100vh;
    width: 100%;
  }

  .pane {
    min-width: 0;
    min-height: 0;
  }

  .feed-list-pane {
    background: var(--color-bg-secondary);
    border-right: 1px solid var(--color-border);
  }

  .timeline-pane {
    background: var(--color-bg);
    border-right: 1px solid var(--color-border);
  }

  .detail-pane {
    background: var(--color-bg);
  }
</style>
