<script lang="ts">
  import { clickOutside } from '$lib/clickOutside';
  import { t } from '$lib/i18n/index.svelte';
  import type { AiContextItem, FeedSummary } from '$lib/types';
  import FeedIcon from './FeedIcon.svelte';
  import Icon from '$lib/icons/Icon.svelte';

  let {
    items,
    scopeLabel = null,
    feeds = [],
    activeItemId = null,
    disabled = false,
    placement = 'below',
    onOpen,
    onRemove,
  }: {
    items: AiContextItem[];
    scopeLabel?: string | null;
    feeds?: FeedSummary[];
    activeItemId?: number | null;
    disabled?: boolean;
    placement?: 'above' | 'below';
    onOpen: (id: number) => void;
    onRemove: (position: number) => void;
  } = $props();

  let expanded = $state(false);
  let trigger = $state<HTMLButtonElement>();
  const scopeText = $derived.by(() => {
    const dates = items.flatMap((i) => (i.publishedAt == null ? [] : [i.publishedAt]));
    const day = (value: number) => new Date(value * 1000).toLocaleDateString();
    return [scopeLabel, dates.length ? `${day(Math.min(...dates))}–${day(Math.max(...dates))}` : '']
      .filter(Boolean)
      .join(' · ');
  });

  function close() {
    expanded = false;
    trigger?.focus();
  }
</script>

<div
  class="context-bar"
  class:above={placement === 'above'}
  use:clickOutside={() => (expanded = false)}
>
  {#if items.length}
    <button
      type="button"
      class="context-trigger"
      bind:this={trigger}
      aria-expanded={expanded}
      aria-haspopup="dialog"
      title={t('ai.contextManage')}
      onkeydown={(event) => {
        if (event.key === 'Escape' && expanded) {
          event.stopPropagation();
          close();
        }
      }}
      onclick={() => (expanded = !expanded)}
    >
      <span class="context-copy">
        <span class="count">{t('ai.contextCount', { count: items.length })}</span>
        <span class="preview">{scopeLabel ?? items[0]?.itemTitle ?? t('ai.deletedArticle')}</span>
      </span>
      <Icon name="chevron-down" size={12} />
    </button>
  {:else}
    <span class="empty">{t('ai.noContext')}</span>
  {/if}
  {#if expanded && items.length}
    <div
      class="context-popover ui-popup"
      role="dialog"
      tabindex="-1"
      aria-label={t('ai.contextManage')}
      onkeydown={(event) => {
        if (event.key === 'Escape') {
          event.stopPropagation();
          close();
        }
      }}
    >
      <div class="popover-heading">
        <strong>{t('ai.contextCount', { count: items.length })}</strong>
        <button type="button" class="close" aria-label={t('ai.contextClose')} onclick={close}
          >×</button
        >
      </div>
      {#if scopeText}<p class="scope">{scopeText}</p>{/if}
      {#each items as item (item.position)}
        <div
          class="context-row ui-list-row"
          class:active={item.itemId !== null && item.itemId === activeItemId}
          class:ui-selected={item.itemId !== null && item.itemId === activeItemId}
        >
          <button
            type="button"
            class="open"
            disabled={item.itemId === null}
            onclick={() => {
              if (item.itemId !== null) onOpen(item.itemId);
              close();
            }}
          >
            <FeedIcon feed={feeds.find((f) => f.id === item.feedId)} />
            <span
              >[{item.position + 1}] {item.itemId === null
                ? t('ai.deletedArticle')
                : item.itemTitle}</span
            >
          </button>
          <button
            type="button"
            class="remove"
            {disabled}
            aria-label={t('ai.removeContext', { title: item.itemTitle ?? '' })}
            onclick={() => onRemove(item.position)}>×</button
          >
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .context-bar {
    position: relative;
    flex-shrink: 0;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg);
  }
  button {
    border: 0;
    font: inherit;
    font-size: var(--text-small);
    color: var(--color-text-secondary);
    background: transparent;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .context-trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    min-height: var(--control-size);
    padding: var(--space-1) 0;
    text-align: left;
  }
  .context-copy {
    display: flex;
    flex: 1;
    min-width: 0;
    align-items: center;
    gap: var(--space-2);
  }
  .count {
    flex-shrink: 0;
    color: var(--color-text);
    font-weight: var(--weight-medium);
  }
  .preview {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .empty {
    font-size: var(--text-small);
    color: var(--color-text-secondary);
  }
  .context-popover {
    position: absolute;
    z-index: 30;
    left: var(--space-2);
    right: var(--space-2);
    top: calc(100% + var(--space-1));
    max-height: 300px;
    overflow: auto;
  }
  .popover-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-left: var(--space-1);
    font-size: var(--text-small);
  }
  .popover-heading strong {
    font-weight: var(--weight-medium);
  }
  .scope {
    margin: var(--space-1);
    font-size: var(--text-caption);
    color: var(--color-text-secondary);
  }
  .open {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    flex: 1;
    padding: var(--space-2) var(--space-1);
    text-align: left;
  }
  .open span {
    overflow-wrap: anywhere;
  }
  .remove,
  .close {
    flex-shrink: 0;
    width: var(--control-size);
    height: var(--control-size);
    border-radius: var(--control-radius);
  }
  .remove:hover:not(:disabled),
  .close:hover,
  .open:hover:not(:disabled) {
    background: var(--color-hover-bg);
  }
  .above {
    background: transparent;
    border-radius: var(--radius-md) var(--radius-md) 0 0;
  }
  .above .context-popover {
    top: auto;
    bottom: calc(100% + var(--space-1));
  }
</style>
