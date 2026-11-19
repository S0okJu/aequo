<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    /** Optional control beside the title (e.g. "Check again"). */
    action?: Snippet;
    /** `danger` frames destructive actions apart from ordinary settings. */
    tone?: 'default' | 'danger';
    children: Snippet;
  }

  let { title, action, tone = 'default', children }: Props = $props();

  const id = $props.id();
</script>

<!-- A titled card of settings rows; sections stack these so related
     choices read as one block. -->
<section class="group" class:danger={tone === 'danger'} aria-labelledby="{id}-title">
  <header>
    <h2 id="{id}-title">{title}</h2>
    {#if action}{@render action()}{/if}
  </header>
  <div class="card">{@render children()}</div>
</section>

<style>
  .group {
    min-width: 0;
    margin: 0;
  }

  header {
    display: flex;
    min-height: 24px;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border);
    background: transparent;
  }

  h2 {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    font-weight: var(--weight-bold);
  }

  .card {
    container: settings-fields / inline-size;
    padding: 0;
    background: var(--color-bg);
  }

  .danger {
    padding: 0 var(--space-3);
    border: 1px solid color-mix(in srgb, var(--color-error) 35%, var(--color-border));
    border-radius: var(--radius-md);
  }
</style>
