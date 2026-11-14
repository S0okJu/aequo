<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';

  interface Item {
    id: number;
    title: string | null;
  }

  interface FeedUpdatedPayload {
    feedId: number;
    newItemCount: number;
  }

  const PAGE_SIZE = 20;

  let items = $state<Item[]>([]);
  let lastEvent = $state<string | null>(null);

  // Re-queries only the visible window of the merged unread timeline
  // (§6.3) - not the whole table, and never on a timer.
  async function refreshUnreadTimeline() {
    items = await invoke<Item[]>('list_items', {
      limit: PAGE_SIZE,
      offset: 0,
      unreadOnly: true,
    });
  }

  onMount(() => {
    refreshUnreadTimeline();

    // Event-driven only (§3.2/§6.2): the backend pushes feed-updated
    // when new items appear; the frontend never polls for changes -
    // no setInterval/setTimeout loop anywhere in this file.
    const unlisten = listen<FeedUpdatedPayload>('feed-updated', async (event) => {
      lastEvent = `feed ${event.payload.feedId}: +${event.payload.newItemCount} item(s)`;
      await refreshUnreadTimeline();
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  });
</script>

<main class="placeholder">
  <h1>Aequo</h1>
  <p>RSS reader — full UI coming in M7.</p>
  <p>Unread items loaded: {items.length}</p>
  {#if lastEvent}
    <p class="event">Last feed-updated event: {lastEvent}</p>
  {/if}
</main>

<style>
  :root {
    font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif;
  }

  .placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    text-align: center;
    color: #1f1f1f;
  }

  .event {
    font-size: 0.85em;
    opacity: 0.7;
  }

  @media (prefers-color-scheme: dark) {
    .placeholder {
      color: #f0f0f0;
    }
  }
</style>
