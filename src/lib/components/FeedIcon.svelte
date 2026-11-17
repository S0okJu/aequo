<script lang="ts">
  import { feedIconUrl } from '$lib/feedIcons.svelte';
  import { feedDisplayName } from '$lib/format';

  interface Props {
    feed:
      { id: number; title: string | null; url: string; customTitle?: string | null } | undefined;
    size?: number;
  }

  let { feed, size = 16 }: Props = $props();

  // Never rendered via {@html} or inline <svg>: an <img> with a data: URI is
  // what keeps a stored SVG icon from running scripts (§3.12).
  const src = $derived(feed ? feedIconUrl(feed.id) : undefined);
  let broken = $state(false);
  $effect(() => {
    void src;
    broken = false;
  });
  const initial = $derived((feed ? feedDisplayName(feed) : '?').slice(0, 1).toUpperCase());
</script>

{#if src && !broken}
  <img
    class="feed-icon"
    {src}
    alt=""
    width={size}
    height={size}
    decoding="async"
    onerror={() => (broken = true)}
  />
{:else}
  <span
    class="feed-icon fallback"
    aria-hidden="true"
    style:width="{size}px"
    style:height="{size}px"
    style:font-size="{Math.round(size * 0.68)}px">{initial}</span
  >
{/if}

<style>
  .feed-icon {
    flex-shrink: 0;
    border-radius: var(--radius-xs);
    object-fit: contain;
  }

  .fallback {
    display: inline-grid;
    place-items: center;
    background: var(--color-border);
    color: var(--color-text-secondary);
    font-weight: var(--weight-bold);
    line-height: 1;
  }
</style>
