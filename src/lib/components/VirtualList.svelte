<script lang="ts" generics="T">
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    items: T[];
    itemHeight: number;
    overscan?: number;
    onEndReached?: () => void;
    children: Snippet<[T, number]>;
  }

  let { items, itemHeight, overscan = 5, onEndReached, children }: Props = $props();

  let container: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(0);

  // Bounds DOM node count to roughly (viewport / itemHeight) + 2*overscan
  // regardless of items.length (M7 item 4) - a fixed itemHeight keeps
  // the index math exact without measuring each row.
  const totalHeight = $derived(items.length * itemHeight);
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / itemHeight) - overscan));
  const endIndex = $derived(
    Math.min(items.length, Math.ceil((scrollTop + viewportHeight) / itemHeight) + overscan),
  );
  const visibleItems = $derived(items.slice(startIndex, endIndex));
  const topPad = $derived(startIndex * itemHeight);
  const bottomPad = $derived(Math.max(0, totalHeight - endIndex * itemHeight));

  function handleScroll() {
    if (!container) return;
    scrollTop = container.scrollTop;

    const distanceToBottom = container.scrollHeight - container.scrollTop - container.clientHeight;
    if (onEndReached && distanceToBottom < itemHeight * 3) {
      onEndReached();
    }
  }

  onMount(() => {
    if (!container) return;
    viewportHeight = container.clientHeight;
    const resizeObserver = new ResizeObserver(() => {
      if (container) viewportHeight = container.clientHeight;
    });
    resizeObserver.observe(container);
    return () => resizeObserver.disconnect();
  });
</script>

<div class="virtual-list" bind:this={container} onscroll={handleScroll}>
  <div style="height: {topPad}px"></div>
  {#each visibleItems as item, i (startIndex + i)}
    {@render children(item, startIndex + i)}
  {/each}
  <div style="height: {bottomPad}px"></div>
</div>

<style>
  .virtual-list {
    height: 100%;
    overflow-y: auto;
  }
</style>
