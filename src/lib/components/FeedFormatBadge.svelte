<script lang="ts">
  import { feedFormatBadge } from '$lib/format';

  interface Props {
    format: string | null;
  }

  let { format }: Props = $props();
  const label = $derived(feedFormatBadge(format));
</script>

{#if label}
  <span class="format-badge">{label}</span>
{/if}

<style>
  .format-badge {
    flex-shrink: 0;
    padding: 0 6px;
    border-radius: var(--radius-pill);
    background: var(--color-border);
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
    font-weight: var(--weight-medium);
    line-height: 16px;
    white-space: nowrap;
  }
</style>
