<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import Spinner from './Spinner.svelte';
  import type { IconName } from '$lib/icons';

  /**
   * The one "a whole pane has nothing to show" shape, for loading, empty and
   * error alike (docs/UNIFIED_UX_MOTION_PLAN.md §6.2 "T3 영역 대체"). The
   * three kinds share a layout so switching between them doesn't reflow the
   * pane, and reuses the existing global `.empty-placeholder` classes
   * (src/routes/+layout.svelte) rather than introducing new CSS.
   */
  interface Props {
    kind: 'loading' | 'empty' | 'error';
    /** Ignored for 'loading' (always Spinner) and 'error' (always the
     * 'error' icon) - only meaningful for 'empty'. */
    icon?: IconName;
    title?: string;
    body?: string;
    /** One primary action, e.g. a retry button for 'error'. */
    action?: Snippet;
  }

  let { kind, icon = 'empty', title, body, action }: Props = $props();
</script>

<div
  class="empty-placeholder"
  role={kind === 'loading' ? 'status' : kind === 'error' ? 'alert' : undefined}
  aria-busy={kind === 'loading' ? true : undefined}
>
  {#if kind === 'loading'}
    <Spinner size={20} announce={false} />
  {:else}
    <span class="empty-placeholder-icon">
      <Icon name={kind === 'error' ? 'error' : icon} size={20} />
    </span>
  {/if}
  {#if title}<h3>{title}</h3>{/if}
  {#if body}<p>{body}</p>{/if}
  {#if action}<div class="action">{@render action()}</div>{/if}
</div>

<style>
  .action {
    margin-top: var(--space-1);
  }
</style>
