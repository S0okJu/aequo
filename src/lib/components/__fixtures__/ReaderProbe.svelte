<script lang="ts">
  import type { Item } from '$lib/types';
  let { item, onToggleSaved }: { item: Item | null; onToggleSaved?: (id: number) => void } =
    $props();
</script>

<div data-testid="reader-probe" data-item-id={item?.id}>{item?.title}</div>
{#if item}<button type="button" onclick={() => onToggleSaved?.(item!.id)}
    >Toggle retained save</button
  >{/if}
