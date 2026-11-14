<script lang="ts">
  import type { Item, FeedSummary } from '$lib/types';

  interface Props {
    item: Item | null;
    feeds: FeedSummary[];
  }

  let { item, feeds }: Props = $props();

  function feedName(feedId: number): string {
    const feed = feeds.find((f) => f.id === feedId);
    return feed?.title ?? feed?.url ?? 'Unknown feed';
  }

  function formatTime(unixSecs: number): string {
    return new Date(unixSecs * 1000).toLocaleString(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short',
    });
  }
</script>

{#if item}
  <article class="detail">
    <h1 class="title">{item.title ?? '(untitled)'}</h1>
    <p class="meta">{feedName(item.feedId)} · {formatTime(item.publishedAt)}</p>
    {#if item.link}
      <!-- External feed URL (arbitrary content, not a SvelteKit app route) -
           resolve() is for internal routes, so it doesn't apply here. -->
      <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
      <a class="link" href={item.link} target="_blank" rel="noreferrer">{item.link}</a>
    {/if}
    <!-- Plain text, not {@html}: sanitizing feed-sourced HTML before
         rendering it as markup is M8's job (ARCHITECTURE.md §9) -
         this avoids shipping an XSS vector in the meantime. -->
    <p class="summary">{item.summary ?? 'No summary available.'}</p>
  </article>
{:else}
  <div class="empty">
    <p>Select an item to read it.</p>
  </div>
{/if}

<style>
  .detail {
    height: 100%;
    overflow-y: auto;
    padding: 20px;
  }

  .title {
    margin: 0 0 4px;
    font-size: 1.3em;
    font-weight: 600;
    color: var(--color-text);
  }

  .meta {
    margin: 0 0 8px;
    font-size: 0.85em;
    color: var(--color-text-tertiary);
  }

  .link {
    display: inline-block;
    margin-bottom: 16px;
    font-size: 0.85em;
    color: var(--color-accent);
    overflow-wrap: anywhere;
  }

  .summary {
    font-size: 0.95em;
    line-height: 1.5;
    color: var(--color-text);
    white-space: pre-wrap;
  }

  .empty {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-text-tertiary);
    font-size: 0.85em;
  }
</style>
