<script lang="ts">
  import { listItems } from '$lib/api';
  import { t } from '$lib/i18n/index.svelte';
  import type { Item, FeedSummary } from '$lib/types';

  let {
    id,
    query,
    currentItem,
    selectedIds,
    feeds = [],
    atLimit = false,
    onSelect,
    onClose,
    onActiveChange,
  }: {
    id: string;
    query: string;
    currentItem: Item | null;
    selectedIds: number[];
    feeds?: FeedSummary[];
    atLimit?: boolean;
    onSelect: (item: Item) => Promise<boolean>;
    onClose: () => void;
    onActiveChange: (id: string | undefined) => void;
  } = $props();

  let results = $state<Item[]>([]);
  let searching = $state(false);
  let failed = $state(false);
  let choosing = $state(false);
  let active = $state(0);
  let retry = $state(0);
  const showThis = $derived(currentItem !== null && 'this'.startsWith(query.toLowerCase()));
  const options = $derived(
    atLimit
      ? []
      : [
          ...(showThis && currentItem ? [{ item: currentItem, isThis: true }] : []),
          ...results
            .filter(
              (item) =>
                !selectedIds.includes(item.id) && (!showThis || item.id !== currentItem?.id),
            )
            .map((item) => ({ item, isThis: false })),
        ],
  );

  $effect(() => {
    const search = query;
    void retry;
    let cancelled = false;
    active = 0;
    results = [];
    failed = false;
    if (atLimit) {
      searching = false;
      return;
    }
    searching = true;
    const timer = setTimeout(async () => {
      try {
        const items = await listItems(search ? { query: search } : {}, 30);
        if (!cancelled) results = items;
      } catch {
        if (!cancelled) failed = true;
      } finally {
        if (!cancelled) searching = false;
      }
    }, 160);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  });

  $effect(() => {
    const index = Math.min(active, options.length - 1);
    onActiveChange(index >= 0 ? `${id}-${index}` : undefined);
  });

  $effect(() => {
    const index = active;
    // Keep the active result visible while focus remains in the textarea.
    document.getElementById(`${id}-${index}`)?.scrollIntoView?.({ block: 'nearest' });
  });

  async function choose(index: number) {
    const option = options[index];
    if (!option || choosing) return;
    choosing = true;
    try {
      await onSelect(option.item);
    } finally {
      choosing = false;
    }
  }

  export function handleKeydown(event: KeyboardEvent): boolean {
    if (event.isComposing) return false;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return true;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (options.length)
        active = (active + (event.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length;
      return true;
    }
    if ((event.key === 'Enter' && !event.shiftKey) || (event.key === 'Tab' && options.length > 0)) {
      event.preventDefault();
      void choose(Math.min(active, options.length - 1));
      return true;
    }
    return false;
  }
</script>

<div class="mention-picker ui-popup">
  <div class="picker-heading">
    <span>{t('ai.mention.choose')}</span><span class="keyboard-hint">↑↓ · Enter</span>
  </div>
  <div
    class="results"
    {id}
    role="listbox"
    aria-label={t('ai.mention.choose')}
    aria-busy={searching || choosing}
  >
    {#each options as option, index (option.item.id)}
      <button
        type="button"
        role="option"
        class="ui-option"
        id="{id}-{index}"
        aria-selected={index === active}
        class:active={index === active}
        disabled={choosing}
        tabindex="-1"
        title={option.item.title ?? ''}
        onmousedown={(event) => event.preventDefault()}
        onclick={() => choose(index)}
      >
        <span class="result-title"
          >{option.isThis ? '@this' : (option.item.title ?? t('ai.mention.untitled'))}</span
        >
        <span class="result-detail"
          >{option.isThis
            ? option.item.title
            : (feeds.find((feed) => feed.id === option.item.feedId)?.title ??
              t('ai.mention.article'))}{selectedIds.includes(option.item.id)
            ? ` · ${t('ai.mention.added')}`
            : ''}</span
        >
      </button>
    {/each}
  </div>
  {#if atLimit}<p class="state" role="status">{t('ai.mention.limit')}</p>
  {:else if searching}<p class="state" role="status">{t('ai.mention.searching')}</p>
  {:else if failed}
    <div class="state" role="status">
      {t('ai.mention.failed')}
      <button
        type="button"
        class="retry"
        onmousedown={(event) => event.preventDefault()}
        onkeydown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            onClose();
          }
        }}
        onclick={() => retry++}>{t('ai.retry')}</button
      >
    </div>
  {:else if !options.length}<p class="state" role="status">
      {t(
        !currentItem && query.toLowerCase() === 'this'
          ? 'ai.mention.noCurrent'
          : 'ai.mention.noResults',
      )}
    </p>{/if}
</div>

<style>
  .mention-picker {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(100% + var(--space-2));
    z-index: 40;
  }
  .picker-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2) var(--space-2);
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
  }
  .keyboard-hint {
    color: var(--color-text-tertiary);
  }
  .results {
    max-height: 240px;
    overflow-y: auto;
  }
  .result-title,
  .result-detail {
    display: block;
    max-width: 100%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .result-title {
    font-size: var(--text-small);
  }
  .result-detail {
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
  }
  .state {
    margin: var(--space-2);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .retry {
    padding: var(--space-1);
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--color-hover-bg);
    color: var(--color-text);
    font: inherit;
    cursor: pointer;
  }
</style>
