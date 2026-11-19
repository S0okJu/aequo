<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  interface Props {
    size?: number;
    /** Set to false when an ancestor already announces the loading state
     * (e.g. StatePlaceholder's own role="status"/aria-busy) so the two
     * don't both fire as separate live regions for one wait. */
    announce?: boolean;
  }

  let { size = 17, announce = true }: Props = $props();
</script>

<span
  role={announce ? 'status' : undefined}
  aria-label={announce ? t('common.loading') : undefined}
  aria-hidden={announce ? undefined : true}
  class="spinner"
  style:width="{size}px"
  style:height="{size}px"
></span>

<style>
  .spinner {
    display: inline-block;
    flex: 0 0 auto;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    /* Shared `spin` keyframe lives in +layout.svelte - see IconButton.svelte. */
    animation: spin var(--motion-loop) linear infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation: none;
    }
  }
</style>
