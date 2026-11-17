<script lang="ts">
  import Icon from '$lib/icons/Icon.svelte';
  import { t } from '$lib/i18n/index.svelte';

  interface Props {
    rect: DOMRect;
    onHighlight: (color: string) => void;
    onMemo: () => void;
    onAskAi: () => void;
  }

  let { rect, onHighlight, onMemo, onAskAi }: Props = $props();

  const COLORS = ['yellow', 'green', 'blue', 'pink', 'purple'] as const;
  const TOOLBAR_WIDTH = 223;
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
  <button
    type="button"
    class="action"
    aria-label={t('annotation.addMemo')}
    title={t('annotation.addMemo')}
    onclick={onMemo}><Icon name="edit" size={14} /></button
  >
  <button
    type="button"
    class="action"
    aria-label={t('annotation.askAi')}
    title={t('annotation.askAi')}
    onclick={onAskAi}><Icon name="ask-ai" size={14} /></button
  >
</div>

<style>
  .selection-toolbar {
    position: fixed;
    z-index: 50;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
    box-shadow: var(--shadow-popover);
  }

  .swatch {
    width: 22px;
    height: 22px;
    border: 1px solid var(--color-border);
    border-radius: 50%;
    cursor: pointer;
  }

  .swatch:hover {
    transform: scale(1.12);
  }

  .divider {
    width: 1px;
    height: 20px;
    background: var(--color-border);
  }

  .action {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font-size: var(--text-body);
    cursor: pointer;
  }

  .action:hover {
    background: var(--color-hover-bg);
    color: var(--color-accent);
  }
</style>
