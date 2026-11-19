<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { SvelteMap } from 'svelte/reactivity';
  import { visibleRowIndices } from '$lib/virtualList';
  import FeedIcon from './FeedIcon.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import IconButton from './IconButton.svelte';
  import type { IconName } from '$lib/icons';
  import SegmentedControl from './SegmentedControl.svelte';
  import Spinner from './Spinner.svelte';
  import {
    articleDateTitle,
    articleDateLabel,
    articleDayKey,
    formatDayHeading,
    feedDisplayName,
  } from '$lib/format';
  import { t } from '$lib/i18n/index.svelte';
  import type { FeedFolder, FeedSummary, Item } from '$lib/types';

  import type { ListPosition } from '$lib/readingCheckpoint';

  interface Props {
    items: Item[];
    onAskView?: () => void;
    feeds: FeedSummary[];
    folders: FeedFolder[];
    loading?: boolean;
    selectedFeedId: number | null;
    selectedFolderId: number | null;
    selectedToday?: boolean;
    selectedStarred?: boolean;
    selectedItemId: number | null;
    hasMore: boolean;
    hasNewer?: boolean;
    loadingPage?: boolean;
    loadError?: string | null;
    viewKey?: string;
    resumePosition?: ListPosition | null;
    unreadOnly?: boolean;
    /** Unread total for the whole view (all pages); falls back to the loaded items. */
    unreadCount?: number | null;
    onSelect: (itemId: number) => void;
    onToggleSaved: (itemId: number) => void;
    onEndReached: () => void | Promise<void>;
    onStartReached?: () => void | Promise<void>;
    onRetry?: () => void | Promise<void>;
    onAnchorChange?: (itemId: number, offset: number) => void;
    onUnreadOnlyChange?: (unreadOnly: boolean) => void;
    onMarkAllRead?: () => void;
    /** Search text for this view; '' when not searching. */
    query?: string;
    onQueryChange?: (query: string) => void;
    onRefresh?: () => void;
    refreshing?: boolean;
  }

  let {
    items,
    onAskView,
    feeds,
    folders,
    loading = false,
    selectedFeedId,
    selectedFolderId,
    selectedToday = false,
    selectedStarred = false,
    selectedItemId,
    hasMore,
    hasNewer = false,
    loadingPage = false,
    loadError = null,
    viewKey = '',
    resumePosition = null,
    unreadOnly = false,
    unreadCount,
    onSelect,
    onToggleSaved,
    onEndReached,
    onStartReached,
    onRetry,
    onAnchorChange,
    onUnreadOnlyChange,
    onMarkAllRead,
    query = '',
    onQueryChange,
    onRefresh,
    refreshing = false,
  }: Props = $props();

  // svelte-ignore state_referenced_locally
  let searchOpen = $state(query !== '');
  let searchInput = $state<HTMLInputElement | undefined>(undefined);

  function openSearch() {
    searchOpen = true;
    queueMicrotask(() => searchInput?.focus());
  }

  function closeSearch() {
    searchOpen = false;
    onQueryChange?.('');
  }

  /** The title split around case-insensitive matches of the search text. */
  function highlight(title: string): { text: string; match: boolean }[] {
    const needle = query.trim().toLowerCase();
    if (!needle) return [{ text: title, match: false }];
    const parts: { text: string; match: boolean }[] = [];
    const haystack = title.toLowerCase();
    let from = 0;
    for (let at = haystack.indexOf(needle); at !== -1; at = haystack.indexOf(needle, from)) {
      if (at > from) parts.push({ text: title.slice(from, at), match: false });
      parts.push({ text: title.slice(at, at + needle.length), match: true });
      from = at + needle.length;
    }
    if (from < title.length) parts.push({ text: title.slice(from), match: false });
    return parts;
  }

  const selectedFeed = $derived(feeds.find((feed) => feed.id === selectedFeedId));
  const selectedFolder = $derived(folders.find((folder) => folder.id === selectedFolderId));
  const viewLabel = $derived(
    t(
      selectedStarred
        ? 'posts.label.starred'
        : selectedToday
          ? 'posts.label.today'
          : selectedFeed
            ? 'posts.label.feed'
            : selectedFolder
              ? 'posts.label.folder'
              : 'posts.label.all',
    ),
  );
  const viewTitle = $derived(
    selectedStarred
      ? t('posts.title.starred')
      : selectedToday
        ? t('posts.title.today')
        : selectedFeed
          ? feedDisplayName(selectedFeed)
          : selectedFolder
            ? selectedFolder.name
            : t('posts.title.all'),
  );
  const viewIcon = $derived<IconName>(
    selectedStarred
      ? 'star-outline'
      : selectedToday
        ? 'clock'
        : selectedFolder
          ? 'folder'
          : 'inbox',
  );
  // Saved posts are kept regardless of read state, so there's nothing to filter.
  const showFilter = $derived(onUnreadOnlyChange !== undefined && !selectedStarred);
  const unreadInView = $derived(
    unreadCount === null ? null : (unreadCount ?? items.filter((item) => !item.isRead).length),
  );

  function feedFor(feedId: number): FeedSummary | undefined {
    return feeds.find((feed) => feed.id === feedId);
  }

  // Shared clock for the relative timestamps ("5 min. ago"), ticking once a
  // minute so they don't go stale while the list stays open.
  let nowSecs = $state(Date.now() / 1000);
  onMount(() => {
    const timer = setInterval(() => (nowSecs = Date.now() / 1000), 60_000);
    return () => clearInterval(timer);
  });

  let scrolled = $state(false);
  let scrollEl = $state<HTMLDivElement>();
  let rowsEl = $state<HTMLDivElement>();
  let headerEl = $state<HTMLElement>();
  let headerHeight = $state(48);
  let viewportHeight = $state(600);
  let viewportTop = $state(0);
  let focusedId = $state<number | null>(null);
  let preservingId = $state<number | null>(null);
  const heights = new SvelteMap<number, number>();
  const offsets = $derived.by(() => {
    const result = [0];
    for (const item of items) result.push(result.at(-1)! + (heights.get(item.id) ?? 140));
    return result;
  });
  const renderIndices = $derived(
    visibleRowIndices(offsets, viewportTop, Math.max(1, viewportHeight - headerHeight), [
      items.findIndex((item) => item.id === focusedId),
      items.findIndex((item) => item.id === preservingId),
    ]),
  );
  let restoreVersion = 0;
  let pendingAnchor: { id: number; offset: number } | null = null;
  let stableAnchor: { id: number; offset: number } | null = null;
  let expectedScrollTop: number | null = null;
  let disposed = false;
  let focusResumePending = false;

  function updateViewport() {
    if (!scrollEl || !rowsEl) return;
    viewportTop = Math.max(
      0,
      scrollEl.getBoundingClientRect().top + headerHeight - rowsEl.getBoundingClientRect().top,
    );
    const anchor = captureAnchor();
    stableAnchor = anchor;
    if (anchor) onAnchorChange?.(anchor.id, anchor.offset);
  }

  function captureAnchor(): { id: number; offset: number } | null {
    if (!scrollEl || !rowsEl) return null;
    const top = scrollEl.getBoundingClientRect().top + headerHeight;
    const bottom = scrollEl.getBoundingClientRect().bottom;
    const row = [...rowsEl.querySelectorAll<HTMLElement>('[data-row-id]')].find(
      (node) =>
        node.getBoundingClientRect().bottom > top && node.getBoundingClientRect().top < bottom,
    );
    return row
      ? { id: Number(row.dataset.rowId), offset: row.getBoundingClientRect().top - top }
      : null;
  }

  // Run before DOM changes, pin the old visible row during the update, then
  // restore its pixel offset even if a page was removed above it.
  $effect.pre(() => {
    void items;
    void offsets;
    void hasNewer;
    const anchor = pendingAnchor ?? stableAnchor ?? captureAnchor();
    const version = ++restoreVersion;
    if (!anchor) return;
    pendingAnchor = anchor;
    preservingId = anchor.id;
    finishAnchorRestore(anchor, version);
  });

  function finishAnchorRestore(anchor: ListPosition, version: number) {
    void tick().then(() => {
      if (disposed || version !== restoreVersion || !scrollEl || !rowsEl) return;
      const row = rowsEl.querySelector<HTMLElement>(`[data-row-id="${anchor.id}"]`);
      if (row) {
        const top = scrollEl.getBoundingClientRect().top + headerHeight;
        setScroll(scrollEl.scrollTop + row.getBoundingClientRect().top - top - anchor.offset);
        if (focusResumePending) {
          row.querySelector<HTMLElement>('.post')?.focus({ preventScroll: true });
          focusedId = anchor.id;
          focusResumePending = false;
        }
      }
      updateViewport();
      pendingAnchor = null;
      preservingId = null;
    });
  }

  $effect(() => {
    const ids = new Set(items.map((item) => item.id));
    untrack(() => {
      for (const id of heights.keys()) if (!ids.has(id)) heights.delete(id);
    });
  });

  let lastViewKey: string | undefined;
  $effect(() => {
    const key = viewKey;
    if (key === lastViewKey) return;
    lastViewKey = key;
    untrack(() => {
      restoreVersion++;
      pendingAnchor = null;
      stableAnchor = null;
      preservingId = null;
      focusedId = null;
      setScroll(0);
      viewportTop = 0;
    });
  });

  let appliedResume: ListPosition | null = null;
  $effect(() => {
    const position = resumePosition;
    const index = position ? items.findIndex((item) => item.id === position.id) : -1;
    if (!position || index < 0 || appliedResume === position) return;
    appliedResume = position;
    untrack(() => {
      const version = ++restoreVersion;
      focusResumePending = true;
      pendingAnchor = position;
      stableAnchor = position;
      preservingId = position.id;
      setScroll(offsets[index]);
      viewportTop = offsets[index];
      finishAnchorRestore(position, version);
    });
  });

  function measureRow(node: HTMLDivElement, id: number) {
    const measure = () => {
      const height = node.getBoundingClientRect().height;
      if (height > 0 && Math.abs(height - (heights.get(id) ?? 0)) > 0.5) heights.set(id, height);
    };
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(node);
    return {
      destroy() {
        observer?.disconnect();
      },
    };
  }

  onMount(() => {
    const resize = () => {
      viewportHeight = scrollEl?.clientHeight || 600;
      headerHeight = headerEl?.clientHeight || 48;
      updateViewport();
    };
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(resize);
    if (scrollEl) observer?.observe(scrollEl);
    if (headerEl) observer?.observe(headerEl);
    window.addEventListener('resize', resize);
    resize();
    return () => {
      disposed = true;
      restoreVersion++;
      observer?.disconnect();
      window.removeEventListener('resize', resize);
    };
  });

  $effect(() => {
    if (scrollEl && rowsEl) untrack(updateViewport);
  });

  async function moveFocus(id: number, delta: number) {
    cancelRestore();
    const index = items.findIndex((item) => item.id === id);
    if (index + delta < 0 && hasNewer) await onStartReached?.();
    else if (index + delta >= items.length && hasMore) await onEndReached();
    const nextIndex = items.findIndex((item) => item.id === id) + delta;
    const next = items[nextIndex];
    if (!next || !scrollEl) return;
    focusedId = next.id;
    setScroll(scrollEl.scrollTop + offsets[nextIndex] - viewportTop);
    updateViewport();
    await tick();
    rowsEl
      ?.querySelector<HTMLElement>(`[data-post-id="${next.id}"]`)
      ?.focus({ preventScroll: true });
  }

  function handleScroll(event: Event) {
    const element = event.currentTarget as HTMLDivElement;
    const programmatic =
      expectedScrollTop !== null && Math.abs(element.scrollTop - expectedScrollTop) < 1;
    expectedScrollTop = null;
    if (!programmatic) cancelRestore();
    scrolled = element.scrollTop > 0;
    updateViewport();
    if (loadingPage || loading || loadError) return;
    if (hasNewer && element.scrollTop < 200) {
      void onStartReached?.();
    } else if (hasMore && element.scrollTop + element.clientHeight >= element.scrollHeight - 500) {
      void onEndReached();
    }
  }

  function setScroll(top: number) {
    if (!scrollEl) return;
    scrollEl.scrollTop = top;
    expectedScrollTop = scrollEl.scrollTop;
  }

  function cancelRestore() {
    restoreVersion++;
    pendingAnchor = null;
    preservingId = null;
  }
</script>

<div class="posts-shell">
  <div class="posts-scroll" bind:this={scrollEl} onscroll={handleScroll}>
    <!-- Two rows that stay put while the list scrolls: what you're looking at,
       then how to filter and act on it (docs/SIDEBAR_SETTINGS_UX_GOAL.md G7). -->
    <!-- One 48px bar like the AI panel's: where you are on the left, how to
       narrow and act on the list on the right. A narrow column (reader or AI
       open) stacks the two halves instead of squeezing the title. -->
    <header class="feed-context" class:scrolled bind:this={headerEl}>
      <div class="bar">
        <div class="view-heading">
          <span class="view-icon" title={viewLabel}>
            {#if selectedFeed}
              <FeedIcon feed={selectedFeed} size={16} />
            {:else}
              <Icon name={viewIcon} size={15} />
            {/if}
          </span>
          <h2 class="view-title" title={viewTitle}>
            <span class="visually-hidden">{viewLabel}: </span>{viewTitle}
          </h2>
          {#if onAskView}<button
              type="button"
              class="ask-view ui-action"
              aria-label={t('posts.askView')}
              title={t('posts.askView')}
              onclick={onAskView}><Icon name="ask-ai" size={13} />{t('posts.askViewAction')}</button
            >{/if}
        </div>

        <div class="tools">
          {#if searchOpen}
            <div class="search">
              <Icon name="search" size={14} />
              <input
                bind:this={searchInput}
                type="search"
                value={query}
                aria-label={t('posts.search')}
                placeholder={t('posts.searchPlaceholder')}
                oninput={(event) => onQueryChange?.(event.currentTarget.value)}
                onkeydown={(event) => {
                  if (event.key === 'Escape') {
                    event.preventDefault();
                    closeSearch();
                  }
                }}
              />
              {#if query.trim() && !loading && !hasMore && !hasNewer}
                <span class="search-count">{t('posts.searchResults', { count: items.length })}</span
                >
              {/if}
              <IconButton
                size="sm"
                iconSize={12}
                icon="close"
                label={t('posts.closeSearch')}
                onclick={closeSearch}
              />
            </div>
          {:else}
            {#if showFilter && onUnreadOnlyChange}
              <SegmentedControl
                size="sm"
                ariaLabel={t('posts.filter.label')}
                value={unreadOnly}
                onChange={onUnreadOnlyChange}
                options={[
                  { value: false, label: t('posts.filter.all') },
                  {
                    value: true,
                    label: t('posts.filter.unread'),
                    badge: unreadInView ?? undefined,
                  },
                ]}
              />
            {/if}
            <div class="actions" class:grouped={showFilter}>
              {#if onQueryChange}
                <IconButton icon="search" label={t('posts.search')} onclick={openSearch} />
              {/if}
              {#if onRefresh}
                <IconButton
                  icon="retry"
                  label={t('posts.refresh')}
                  spinning={refreshing}
                  disabled={refreshing}
                  onclick={onRefresh}
                />
              {/if}
              {#if onMarkAllRead}
                <IconButton
                  icon="check-all"
                  label={t('posts.markAllRead')}
                  disabled={unreadInView === 0 || unreadInView === null}
                  onclick={onMarkAllRead}
                />
              {/if}
            </div>
          {/if}
        </div>
      </div>
    </header>
    <div class="posts">
      {#if loadError}
        <p class="load-error" role="alert">
          {loadError === 'cacheBudget'
            ? t('posts.cacheBudget')
            : t('posts.loadFailed', { error: loadError })}
        </p>
        <button class="page-button" type="button" onclick={() => void onRetry?.()}
          >{t('posts.retry')}</button
        >
      {/if}
      {#if hasNewer}
        <button
          class="page-button"
          type="button"
          disabled={loadingPage}
          onclick={() => void onStartReached?.()}>{t('posts.loadNewer')}</button
        >
      {/if}
      {#if loading && items.length === 0}
        <div class="empty-placeholder">
          <Spinner size={20} />
        </div>
      {:else if items.length === 0 && !loadError}
        <div class="empty-placeholder">
          <span class="empty-placeholder-icon"
            ><Icon name={selectedStarred ? 'star-outline' : 'empty'} size={20} /></span
          >
          {#if query.trim()}
            <h1>{t('posts.empty.noMatchTitle')}</h1>
            <p>{t('posts.empty.noMatchBody', { query: query.trim() })}</p>
          {:else if selectedStarred}
            <h1>{t('posts.empty.starredTitle')}</h1>
            <p>{t('posts.empty.starredBody')}</p>
          {:else if unreadOnly}
            <h1>{t('posts.empty.caughtUpTitle')}</h1>
            <p>{t('posts.empty.caughtUpBody')}</p>
          {:else}
            <h1>{t('posts.empty.noneTitle')}</h1>
            <p>{t('posts.empty.noneBody')}</p>
          {/if}
        </div>
      {:else}
        <div class="rows" bind:this={rowsEl}>
          {#each renderIndices as index, position (items[index].id)}
            {@const item = items[index]}
            {@const day = articleDayKey(item)}
            {@const previous = index > 0 ? articleDayKey(items[index - 1]) : null}
            {@const feed = feedFor(item.feedId)}
            {@const feedName = feed ? feedDisplayName(feed) : t('common.unknownFeed')}
            {@const gap =
              offsets[index] - (position > 0 ? offsets[renderIndices[position - 1] + 1] : 0)}
            {#if gap > 0}<div aria-hidden="true" style:height={`${gap}px`}></div>{/if}
            <div class="post-row" data-row-id={item.id} use:measureRow={item.id}>
              {#if !previous || day !== previous}
                <h2 class="date-heading">
                  {formatDayHeading(
                    item.publishedAt,
                    nowSecs,
                    undefined,
                    item.publishedAtKind === 'date',
                  )}
                </h2>
              {/if}
              <div
                class="post"
                data-post-id={item.id}
                class:separated={previous && day === previous}
                class:selected={selectedItemId === item.id}
                class:unread={!item.isRead}
                role="button"
                tabindex="0"
                onclick={() => onSelect(item.id)}
                onfocusin={() => (focusedId = item.id)}
                onfocusout={(event) => {
                  if (!event.currentTarget.contains(event.relatedTarget as Node | null))
                    focusedId = null;
                }}
                onkeydown={(event) => {
                  if (
                    event.target === event.currentTarget &&
                    (event.key === 'ArrowDown' || event.key === 'ArrowUp')
                  ) {
                    event.preventDefault();
                    void moveFocus(item.id, event.key === 'ArrowDown' ? 1 : -1);
                  }
                  if (
                    event.target === event.currentTarget &&
                    (event.key === 'Enter' || event.key === ' ')
                  ) {
                    event.preventDefault();
                    onSelect(item.id);
                  }
                }}
              >
                <span class="post-header">
                  <FeedIcon {feed} />
                  <span class="source">{feedName}</span>
                  <span class="separator" aria-hidden="true">·</span>
                  <time
                    datetime={item.publishedAtKind === 'date'
                      ? new Date(item.publishedAt * 1000).toISOString().slice(0, 10)
                      : new Date(item.publishedAt * 1000).toISOString()}
                    title={articleDateTitle(item)}
                  >
                    {articleDateLabel(item, nowSecs)}
                  </time>
                  {#if !item.isRead}<span class="visually-hidden">{t('posts.unreadMarker')}</span
                    >{/if}
                  <button
                    class="save-button"
                    class:saved={item.isSaved}
                    type="button"
                    aria-label={item.isSaved ? t('common.unsavePost') : t('common.savePost')}
                    title={item.isSaved ? t('common.saved') : t('common.save')}
                    onclick={(event) => {
                      event.stopPropagation();
                      onToggleSaved(item.id);
                    }}><Icon name={item.isSaved ? 'star' : 'star-outline'} size={14} /></button
                  >
                </span>
                <span class="title"
                  >{#each highlight(item.title ?? '(untitled)') as part, index (index)}{#if part.match}<mark
                        >{part.text}</mark
                      >{:else}{part.text}{/if}{/each}</span
                >
                {#if item.preview}<span class="preview">{item.preview}</span>{/if}
              </div>
            </div>
          {/each}
          {#if offsets.at(-1)! > (renderIndices.length ? offsets[renderIndices.at(-1)! + 1] : 0)}
            <div
              aria-hidden="true"
              style:height={`${offsets.at(-1)! - (renderIndices.length ? offsets[renderIndices.at(-1)! + 1] : 0)}px`}
            ></div>
          {/if}
        </div>
        {#if hasMore}
          <button
            class="page-button"
            type="button"
            disabled={loadingPage}
            onclick={() => void onEndReached()}>{t('posts.loadOlder')}</button
          >
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .posts-shell {
    position: relative;
    height: 100%;
    min-height: 0;
    isolation: isolate;
  }
  .post-row {
    display: flow-root;
  }
  .rows {
    width: 100%;
    overflow-anchor: none;
  }
  .page-button {
    min-height: 32px;
    margin: var(--space-2) var(--space-3);
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--color-bg-secondary);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }
  .load-error {
    margin: var(--space-3);
    color: var(--color-error);
    font-size: var(--text-small);
  }
  .preview {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .posts-scroll {
    display: flex;
    height: 100%;
    flex-direction: column;
    overflow-y: auto;
    overscroll-behavior: contain;
    overflow-anchor: none;
    container-type: inline-size;
  }

  .posts {
    display: flex;
    width: 100%;
    flex: 1 0 auto;
    flex-direction: column;
    max-width: var(--pane-content-max-width);
    margin: 0 auto;
    padding: 0 var(--space-2) var(--space-5);
  }

  .posts > .empty-placeholder {
    flex: 1;
  }

  .feed-context {
    position: sticky;
    z-index: 2;
    top: 0;
    flex-shrink: 0;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg);
  }

  /* Once the list slides underneath, a soft shadow lifts the bar off it. */
  .feed-context.scrolled {
    box-shadow: 0 6px 12px -10px rgba(0, 0, 0, 0.25);
  }

  .bar {
    display: flex;
    min-height: 48px;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
    max-width: var(--pane-content-max-width);
    margin: 0 auto;
    padding: var(--space-2) calc(var(--space-2) + var(--space-3));
  }

  .view-heading {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: var(--space-2);
  }

  .view-icon {
    display: grid;
    width: var(--space-4);
    flex-shrink: 0;
    place-items: center;
    color: var(--color-text-secondary);
  }

  .view-title {
    min-width: 0;
    margin: 0;
    overflow: hidden;
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tools {
    display: flex;
    min-height: 28px;
    flex-shrink: 0;
    align-items: center;
    gap: var(--space-2);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-right: calc(var(--space-1) * -1);
  }

  /* Filter and actions read as two groups. */
  .actions.grouped {
    padding-left: var(--space-2);
    border-left: 1px solid var(--color-border);
  }

  /* Narrow column: the tools take their own full-width row under the title. */
  @container (max-width: 440px) {
    .bar {
      padding-top: var(--space-3);
    }

    .view-heading {
      flex-basis: 100%;
    }

    .tools {
      flex: 1;
    }

    .tools :global(.segmented) {
      flex: 1;
    }

    .tools :global(.segmented button) {
      flex: 1 1 0;
    }

    .actions {
      margin-left: auto;
    }
  }

  .search {
    display: flex;
    width: 260px;
    max-width: 100%;
    min-width: 0;
    min-height: 28px;
    flex: 1;
    /* Keyframe is declared globally in +layout.svelte (-global-search-in)
       so +page.svelte's sidebar filter pill can reuse it too. */
    animation: search-in var(--motion-base) var(--ease-enter) both;
    align-items: center;
    gap: var(--space-2);
    /* The 2px is tuned against the close IconButton's own internal padding
       for optical centering, not generic layout spacing - left raw. */
    padding: 0 2px 0 var(--space-2);
    border: 1px solid var(--color-accent);
    border-radius: var(--radius-md);
    color: var(--color-text-tertiary);
  }
  .search input {
    min-width: 0;
    flex: 1;
    height: 28px;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
  }
  .search input:focus {
    outline: none;
  }
  .search input::-webkit-search-cancel-button {
    display: none;
  }

  .search-count {
    flex-shrink: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .title mark {
    border-radius: var(--radius-xs);
    background: var(--hl-yellow);
    color: inherit;
  }

  .date-heading {
    margin: var(--space-4) var(--space-3) var(--space-1);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    font-weight: var(--weight-medium);
  }

  /* A flat row separated by a hairline instead of a bordered card - the
     unread state is carried by title weight, not a colored dot. */
  .post {
    position: relative;
    display: block;
    width: 100%;
    padding: var(--space-3);
    border-radius: var(--radius-md);
    color: var(--color-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--transition);
  }

  .post.separated::before {
    position: absolute;
    top: 0;
    right: var(--space-3);
    left: var(--space-3);
    height: 1px;
    background: var(--color-border);
    content: '';
  }

  .post:hover {
    background: var(--color-hover-bg);
  }

  .post.selected {
    background: var(--color-selection-bg);
  }

  /* Hide the hairline on either side of a highlighted row so the hover /
     selection fill reads as one shape. */
  .post:hover::before,
  .post.selected::before {
    display: none;
  }

  .post:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
  }

  .post-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    height: 20px;
    margin-bottom: var(--space-1);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .source {
    min-width: 0;
    overflow: hidden;
    color: var(--color-text-secondary);
    font-weight: var(--weight-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .separator,
  time {
    flex-shrink: 0;
  }

  .save-button {
    display: inline-flex;
    width: var(--control-size-xs);
    height: var(--control-size-xs);
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    margin-left: auto;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
    opacity: 0;
  }

  /* Only a saved post shows its star at rest; others reveal it on hover or
     keyboard focus so the control stays reachable. */
  .save-button.saved,
  .post:hover .save-button,
  .post:focus-within .save-button,
  .post.selected .save-button {
    opacity: 1;
  }

  .save-button:hover,
  .save-button.saved {
    color: var(--color-accent);
  }

  .title {
    display: block;
    display: -webkit-box;
    /* Repeats per row in a long scroll list - left raw rather than doubled
       to --space-1, which would visibly loosen every post's title block. */
    margin-bottom: 2px;
    overflow: hidden;
    color: var(--color-text-secondary);
    font-size: var(--text-title);
    font-weight: var(--weight-regular);
    line-height: 1.4;
    word-break: keep-all;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }

  .post.unread .title {
    color: var(--color-text);
    font-weight: var(--weight-bold);
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
