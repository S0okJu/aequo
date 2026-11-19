<script lang="ts">
  import IconButton from './IconButton.svelte';
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    rect: DOMRect;
    onHighlight: (color: string) => void;
    onMemo: () => void;
    onAskAi: () => void;
  }

  let { rect, onHighlight, onMemo, onAskAi }: Props = $props();

  const COLORS = ['yellow', 'green', 'blue', 'pink', 'purple'] as const;
  // +20px over the old 22px swatches: same 26px swatch size as
  // AnnotationPopover.svelte now (docs/UNIFIED_UX_MOTION_PLAN.md §7.5 D3).
  const TOOLBAR_WIDTH = 243;
  const TOOLBAR_HEIGHT = 42;
  const MARGIN = 8;

  // Prefer floating just above the selection; clamp so it never runs off
  // any viewport edge (the reader pane can be as narrow as ~300px).
  const top = $derived(
    Math.max(
      MARGIN,
      Math.min(rect.top - TOOLBAR_HEIGHT - MARGIN, window.innerHeight - TOOLBAR_HEIGHT - MARGIN),
    ),
  );
  const left = $derived(
    Math.max(
      MARGIN,
      Math.min(
        rect.left + rect.width / 2 - TOOLBAR_WIDTH / 2,
        window.innerWidth - TOOLBAR_WIDTH - MARGIN,
      ),
    ),
  );
</script>

<div
  class="selection-toolbar"
  style:top="{top}px"
  style:left="{left}px"
  role="toolbar"
  aria-label={t('annotation.toolbar')}
  tabindex="-1"
  onmousedown={(event) => event.preventDefault()}
>
  {#each COLORS as color (color)}
    <button
      type="button"
      class="swatch"
      style:background="var(--hl-{color})"
      aria-label={t('annotation.highlightColor', { color: t(`color.${color}`) })}
      title={t('annotation.highlightColor', { color: t(`color.${color}`) })}
      onclick={() => onHighlight(color)}
    ></button>
  {/each}
  <span class="divider" aria-hidden="true"></span>
  <IconButton icon="edit" label={t('annotation.addMemo')} onclick={onMemo} />
  <IconButton icon="ask-ai" label={t('annotation.askAi')} onclick={onAskAi} />
</div>

<style>
  .selection-toolbar {
    position: fixed;
    z-index: 50;
    display: flex;
    align-items: center;
    /* Smaller than the general --space-2 rhythm on purpose - this is the
       app's most compact floating control, a one-line selection popup. */
    gap: var(--space-1);
    padding: var(--space-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
    box-shadow: var(--shadow-popover);
  }

  .swatch {
    width: 26px;
    height: 26px;
    border: 1px solid var(--color-border);
    border-radius: 50%;
    cursor: pointer;
    transition: border-color var(--transition);
  }

  .swatch:hover {
    border-color: var(--color-accent);
  }

  .divider {
    width: 1px;
    height: 20px;
    background: var(--color-border);
  }
</style>
