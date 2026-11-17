<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { listen } from '@tauri-apps/api/event';
  import type AiPanelType from '$lib/components/AiPanel.svelte';
  import FeedList from '$lib/components/FeedList.svelte';
  import PostsFeed from '$lib/components/PostsFeed.svelte';
  import PostReader from '$lib/components/PostReader.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import Spinner from '$lib/components/Spinner.svelte';
  import { showToast } from '$lib/toast.svelte.ts';
  import { t } from '$lib/i18n/index.svelte';
  import {
    listFeeds,
    listFeedFolders,
    listItems,
    markItemsRead,
    markItemsUnread,
    markRead,
    setItemSaved,
    updateFeedOrganization,
  } from '$lib/api';
  import type { FeedFolder, FeedSummary, Item, ItemFilter } from '$lib/types';

  const PAGE_SIZE = 50;
  const REFRESH_DEBOUNCE_MS = 200;
  const AI_PANEL_OPEN_KEY = 'aequo:aiPanelOpen';
  const SIDEBAR_WIDTH_KEY = 'aequo:sidebarWidth';
  const UNREAD_ONLY_KEY = 'aequo:unreadOnly';

  function readStoredUnreadOnly(): boolean {
    try {
      return localStorage.getItem(UNREAD_ONLY_KEY) === 'true';
    } catch {
      return false;
    }
  }

  function readStoredAiPanelOpen(): boolean {
    try {
      const stored = localStorage.getItem(AI_PANEL_OPEN_KEY);
      if (stored !== null) return stored === 'true';
    } catch {
      // Storage may be unavailable (private mode, etc.) - default to closed.
    }
    return false;
  }

  function initialViewportWidth(): number {
    try {
      // Match the width the CSS layout can actually use. innerWidth includes
      // the vertical scrollbar in desktop WebViews, which can make the fixed
      // grid tracks exceed the page by the scrollbar width.
      return document.documentElement.clientWidth || window.innerWidth;
    } catch {
      return 1360;
    }
  }

  function readStoredSidebarWidth(): number {
    try {
      const stored = Number(localStorage.getItem(SIDEBAR_WIDTH_KEY));
      if (Number.isFinite(stored) && stored > 0) return Math.min(360, Math.max(160, stored));
    } catch {
      // Storage may be unavailable - use the default width.
    }
    return 220;
  }

  interface FeedUpdatedPayload {
    feedId: number;
    newItemCount: number;
  }

  let feeds = $state<FeedSummary[]>([]);
  let folders = $state<FeedFolder[]>([]);
  let items = $state<Item[]>([]);
  let initialLoading = $state(true);
  let hasMoreItems = $state(true);
  let loadingMoreItems = false; // plain guard against overlapping loads, not UI state
  let selectedFeedId = $state<number | null>(null);
  let selectedFolderId = $state<number | null>(null);
  let selectedToday = $state(false);
  let selectedStarred = $state(false);
  let selectedItemId = $state<number | null>(null);
  let unreadOnly = $state(readStoredUnreadOnly());
  // When the current view's first page was loaded (unix seconds): "mark all
  // read" only touches items fetched by then, never ones that arrived since.
  let viewLoadedAt = 0;
  let viewportWidth = $state(initialViewportWidth());
  let sidebarWidth = $state(readStoredSidebarWidth());
  let readerWidth = $state(500);
  let aiWidth = $state(400);
  let aiPanelOpen = $state(readStoredAiPanelOpen());
  // Loaded on demand the first time the panel opens, so the ~1500-line
  // AiPanel component isn't parsed/compiled into the main chunk for
  // sessions that keep it collapsed.
  let AiPanelComponent = $state<typeof AiPanelType | null>(null);
  let isDragging = $state(false);
  // One-shot: set when the reader's "Ask AI" selection toolbar button is
  // clicked, consumed by AiPanel to prefill a new session's draft, then
  // cleared (docs/READER_ANNOTATIONS_PLAN.md §4.3). PostReader and AiPanel
  // are sibling panes here, not parent/child, so this has to live here.
  let pendingQuote = $state<string | null>(null);
  const postListWidth = $derived(Math.min(270, Math.max(220, Math.round(viewportWidth * 0.2))));

  const selectedItem = $derived(items.find((item) => item.id === selectedItemId) ?? null);

  // Every grid track below is a plain pixel number (never a mix of fixed
  // and `minmax(...,1fr)` shapes) so `.layout`'s grid-template-columns
  // transition can interpolate smoothly instead of snapping - browsers
  // can't animate between differently-shaped track definitions.
  const showReader = $derived(selectedItem !== null);
  // The AI panel belongs to the open post (§2): it only takes a track while
  // a post is open, and `aiPanelOpen` remembers whether to bring it back
  // with the next post.
  const showAi = $derived(aiPanelOpen && showReader);
  const readerHandleWidth = $derived(showReader ? 6 : 0);
  const aiHandleWidth = $derived(showAi ? 6 : 0);
  const minimumReaderWidth = $derived(showReader ? 320 : 0);
  const minimumAiWidth = $derived(showAi ? 300 : 0);
  const layoutSidebarWidth = $derived(
    Math.min(
      sidebarWidth,
      Math.max(
        160,
        viewportWidth -
          postListWidth -
          minimumReaderWidth -
          minimumAiWidth -
          6 -
          readerHandleWidth -
          aiHandleWidth,
      ),
    ),
  );
  const effectiveAiWidth = $derived(
    showAi
      ? Math.min(
          aiWidth,
          Math.max(
            300,
            viewportWidth -
              layoutSidebarWidth -
              6 -
              postListWidth -
              minimumReaderWidth -
              readerHandleWidth -
              aiHandleWidth,
          ),
        )
      : 0,
  );
  const readerPaneWidth = $derived(
    showReader
      ? Math.min(
          readerWidth,
          Math.max(
            320,
            viewportWidth -
              layoutSidebarWidth -
              6 -
              postListWidth -
              effectiveAiWidth -
              readerHandleWidth -
              aiHandleWidth,
          ),
        )
      : 0,
  );
  const postListComputedWidth = $derived(
    Math.max(
      postListWidth,
      viewportWidth -
        layoutSidebarWidth -
        6 -
        readerHandleWidth -
        readerPaneWidth -
        aiHandleWidth -
        effectiveAiWidth,
    ),
  );

  $effect(() => {
    try {
      localStorage.setItem(AI_PANEL_OPEN_KEY, String(aiPanelOpen));
    } catch {
      // Storage may be unavailable - the preference just won't persist.
    }
  });

  $effect(() => {
    if (showAi && !AiPanelComponent) {
      import('$lib/components/AiPanel.svelte').then((module) => {
        AiPanelComponent = module.default;
      });
    }
  });

  $effect(() => {
    try {
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(sidebarWidth));
    } catch {
      // The current width still works if the preference cannot be persisted.
    }
  });

  async function refreshFeeds() {
    const [nextFeeds, nextFolders] = await Promise.all([listFeeds(), listFeedFolders()]);
    feeds = nextFeeds;
    folders = nextFolders;
  }

  // The filter for whatever the sidebar has selected. Starred ignores the
  // unread-only toggle: saved posts are kept on purpose, read or not.
  function currentFilter(): ItemFilter {
    const filter: ItemFilter = { unreadOnly: unreadOnly && !selectedStarred };
    if (selectedStarred) filter.savedOnly = true;
    if (selectedFeedId !== null) filter.feedId = selectedFeedId;
    if (selectedFolderId !== null) filter.folderId = selectedFolderId;
    if (selectedToday) {
      const now = new Date();
      filter.publishedFrom = Math.floor(
        new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000,
      );
      filter.publishedTo = Math.floor(
        new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime() / 1000,
      );
    }
    return filter;
  }

  // First page of the merged timeline (§6.3) for the current selection -
  // replaces items entirely (a feed-updated event or a selection change can
  // shift what "page 1" even means). A background refresh in unread-only
  // mode keeps posts the user already read in place, so a post doesn't
  // vanish from under them mid-read; they drop out on the next view change.
  async function refreshItems(background = false) {
    const loadedAt = Math.floor(Date.now() / 1000);
    const firstPage = await listItems(currentFilter(), PAGE_SIZE);
    if (background && unreadOnly && !selectedStarred) {
      const fetchedIds = new Set(firstPage.map((item) => item.id));
      const keptRead = items.filter((item) => item.isRead && !fetchedIds.has(item.id));
      items = [...firstPage, ...keptRead].sort(
        (a, b) => b.publishedAt - a.publishedAt || b.id - a.id,
      );
    } else {
      items = firstPage;
    }
    hasMoreItems = firstPage.length === PAGE_SIZE;
    viewLoadedAt = loadedAt;
    if (selectedItemId !== null && !items.some((item) => item.id === selectedItemId)) {
      selectedItemId = null;
    }
  }

  function setUnreadOnly(value: boolean) {
    if (unreadOnly === value) return;
    unreadOnly = value;
    try {
      localStorage.setItem(UNREAD_ONLY_KEY, String(value));
    } catch {
      // The toggle still applies for this session.
    }
    refreshItems();
  }

  // Unread total for the current view, from the per-feed counts (which cover
  // every page, not just the loaded ones). Today has no per-feed breakdown,
  // so it falls back to the loaded items.
  const viewUnreadCount = $derived.by(() => {
    if (selectedToday || selectedStarred) return items.filter((item) => !item.isRead).length;
    const inView = feeds.filter((feed) =>
      selectedFeedId !== null
        ? feed.id === selectedFeedId
        : selectedFolderId !== null
          ? feed.folderId === selectedFolderId
          : !feed.hiddenFromAll,
    );
    return inView.reduce((sum, feed) => sum + feed.unreadCount, 0);
  });

  async function markAllRead() {
    let ids: number[];
    try {
      ids = await markItemsRead({ ...currentFilter(), unreadOnly: false }, viewLoadedAt);
    } catch (error) {
      showToast(t('toast.markAllReadFailed'), { type: 'error' });
      console.error('mark_items_read failed:', error);
      return;
    }
    if (ids.length === 0) return;
    const marked = new Set(ids);
    for (const item of items) if (marked.has(item.id)) item.isRead = true;
    await refreshFeeds();
    showToast(t('toast.markedRead', { count: ids.length }), {
      action: { label: t('common.undo'), run: () => undoMarkAllRead(ids) },
    });
  }

  async function undoMarkAllRead(ids: number[]) {
    try {
      await markItemsUnread(ids);
    } catch (error) {
      showToast(t('toast.undoFailed'), { type: 'error' });
      console.error('mark_items_unread failed:', error);
      return;
    }
    const restored = new Set(ids);
    for (const item of items) if (restored.has(item.id)) item.isRead = false;
    await refreshFeeds();
  }

  // The post feed requests another page as the reader nears the bottom.
  async function loadMoreItems() {
    if (loadingMoreItems || !hasMoreItems || refreshItemsInFlight) return;
    loadingMoreItems = true;
    try {
      const cursor = items[items.length - 1];
      const nextPage = await listItems(currentFilter(), PAGE_SIZE, {
        publishedAt: cursor.publishedAt,
        id: cursor.id,
      });
      items = [...items, ...nextPage];
      hasMoreItems = nextPage.length === PAGE_SIZE;
    } finally {
      loadingMoreItems = false;
    }
  }

  function selectFeed(feedId: number | null) {
    selectedFeedId = feedId;
    selectedFolderId = null;
    selectedToday = false;
    selectedStarred = false;
    selectedItemId = null;
    refreshItems();
  }

  function selectFolder(folderId: number | null) {
    selectedFeedId = null;
    selectedFolderId = folderId;
    selectedToday = false;
    selectedStarred = false;
    selectedItemId = null;
    refreshItems();
  }

  function selectToday() {
    selectedFeedId = null;
    selectedFolderId = null;
    selectedToday = true;
    selectedStarred = false;
    selectedItemId = null;
    refreshItems();
  }

  function selectStarred() {
    selectedFeedId = null;
    selectedFolderId = null;
    selectedToday = false;
    selectedStarred = true;
    selectedItemId = null;
    refreshItems();
  }

  async function moveFeed(feedId: number, folderId: number | null) {
    const feed = feeds.find((f) => f.id === feedId);
    if (!feed || feed.folderId === folderId) return;
    await updateFeedOrganization(feedId, folderId, feed.hiddenFromAll);
    await refreshFeeds();
  }

  // Marking a post read updates the badge immediately, then persists it.
  async function markPostRead(itemId: number) {
    const item = items.find((i) => i.id === itemId);
    if (!item || item.isRead) return;

    item.isRead = true;
    const feed = feeds.find((f) => f.id === item.feedId);
    if (feed) feed.unreadCount = Math.max(0, feed.unreadCount - 1);

    try {
      await markRead(itemId);
    } catch (err) {
      item.isRead = false;
      if (feed) feed.unreadCount += 1;
      showToast(t('toast.markReadFailed'), { type: 'error' });
      console.error('mark_read failed:', err);
    }
  }

  function selectPost(itemId: number) {
    selectedItemId = selectedItemId === itemId ? null : itemId;
    markPostRead(itemId);
  }

  async function toggleSaved(itemId: number) {
    const item = items.find((entry) => entry.id === itemId);
    if (!item) return;
    const previous = item.isSaved;
    item.isSaved = !previous;
    // Unstarring while viewing the Starred tab should drop the item from
    // view immediately, same as how it'd vanish on the next refresh.
    if (selectedStarred && !item.isSaved) {
      items = items.filter((entry) => entry.id !== itemId);
    }
    try {
      await setItemSaved(itemId, item.isSaved);
    } catch (error) {
      item.isSaved = previous;
      if (selectedStarred && previous) {
        items = [...items, item].sort((a, b) => b.publishedAt - a.publishedAt || b.id - a.id);
      }
      showToast(t('toast.saveFailed'), { type: 'error' });
      console.error('set_item_saved failed:', error);
    }
  }

  function resizeReader(event: PointerEvent) {
    if (event.buttons !== 1) return;
    readerWidth = clampReaderWidth(viewportWidth - effectiveAiWidth - event.clientX - 12);
  }

  function clampReaderWidth(width: number) {
    const maxWidth = Math.max(
      320,
      Math.min(760, viewportWidth - layoutSidebarWidth - 6 - postListWidth - effectiveAiWidth - 12),
    );
    return Math.min(maxWidth, Math.max(320, width));
  }

  function clampSidebarWidth(width: number) {
    const minWidth = 160;
    const maxWidth = Math.max(
      minWidth,
      Math.min(360, viewportWidth - 220 - (showReader ? 320 : 0) - effectiveAiWidth - 18),
    );
    return Math.min(maxWidth, Math.max(minWidth, width));
  }

  function resizeSidebar(event: PointerEvent) {
    if (event.buttons !== 1) return;
    sidebarWidth = clampSidebarWidth(event.clientX);
  }

  function resizeSidebarWithKey(event: KeyboardEvent) {
    const step = event.shiftKey ? 48 : 24;
    if (event.key === 'ArrowLeft') {
      event.preventDefault();
      sidebarWidth = clampSidebarWidth(layoutSidebarWidth - step);
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      sidebarWidth = clampSidebarWidth(layoutSidebarWidth + step);
    }
  }

  function resizeAiPanel(event: PointerEvent) {
    if (event.buttons !== 1) return;
    resizeAiPanelTo(viewportWidth - event.clientX);
  }

  function resizeAiPanelWithKey(event: KeyboardEvent) {
    const step = event.shiftKey ? 48 : 24;
    if (event.key === 'ArrowLeft') {
      event.preventDefault();
      resizeAiPanelTo(effectiveAiWidth + step);
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      resizeAiPanelTo(effectiveAiWidth - step);
    }
  }

  function resizeAiPanelTo(width: number) {
    const minReaderWidth = showReader ? 320 : 0;
    const maxWidth = Math.max(
      300,
      Math.min(560, viewportWidth - layoutSidebarWidth - 6 - postListWidth - minReaderWidth - 12),
    );
    aiWidth = Math.min(maxWidth, Math.max(300, width));
    if (showReader) {
      readerWidth = clampReaderWidth(
        viewportWidth - layoutSidebarWidth - 6 - postListWidth - aiWidth - 12,
      );
    }
  }

  function beginReaderResize(event: PointerEvent) {
    event.preventDefault();
    isDragging = true;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function endResize() {
    isDragging = false;
  }

  function resizeReaderWithKey(event: KeyboardEvent) {
    if (event.key === 'ArrowLeft') {
      event.preventDefault();
      readerWidth = clampReaderWidth(readerPaneWidth + 24);
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      readerWidth = clampReaderWidth(readerPaneWidth - 24);
    }
  }

  let refreshFeedsInFlight = false;
  let refreshItemsInFlight = false;
  let scheduledRefreshTimer: ReturnType<typeof setTimeout> | undefined;

  // Coalesces a burst of `feed-updated` events (e.g. several feeds refreshing
  // around the same time) into a single trailing refresh instead of firing
  // one overlapping refresh per event.
  function scheduleBackgroundRefresh() {
    clearTimeout(scheduledRefreshTimer);
    scheduledRefreshTimer = setTimeout(() => {
      if (!refreshFeedsInFlight) {
        refreshFeedsInFlight = true;
        refreshFeeds().finally(() => (refreshFeedsInFlight = false));
      }
      if (!refreshItemsInFlight) {
        refreshItemsInFlight = true;
        refreshItems(true).finally(() => (refreshItemsInFlight = false));
      }
    }, REFRESH_DEBOUNCE_MS);
  }

  onMount(() => {
    const updateViewport = () => {
      viewportWidth = document.documentElement.clientWidth || window.innerWidth;
    };
    updateViewport();
    window.addEventListener('resize', updateViewport);
    Promise.all([refreshFeeds(), refreshItems()]).finally(() => {
      initialLoading = false;
    });

    // Event-driven only (§3.2/§6.2) - no polling timer anywhere here.
    const unlisten = listen<FeedUpdatedPayload>('feed-updated', scheduleBackgroundRefresh);

    return () => {
      window.removeEventListener('resize', updateViewport);
      clearTimeout(scheduledRefreshTimer);
      unlisten.then((fn) => fn());
    };
  });
</script>

<div
  class="layout"
  class:no-transition={isDragging}
  style={`grid-template-columns: ${layoutSidebarWidth}px 6px ${postListComputedWidth}px ${readerHandleWidth}px ${readerPaneWidth}px ${aiHandleWidth}px ${effectiveAiWidth}px`}
>
  <aside class="pane feed-list-pane">
    <div class="feed-list-scroll">
      <FeedList
        {feeds}
        {folders}
        loading={initialLoading}
        {selectedFeedId}
        {selectedFolderId}
        {selectedToday}
        {selectedStarred}
        {unreadOnly}
        onSelectFeed={selectFeed}
        onSelectFolder={selectFolder}
        onSelectToday={selectToday}
        onSelectStarred={selectStarred}
        onManageFolders={() => goto(resolve('/settings?tab=folders'))}
        onMoveFeed={moveFeed}
      />
    </div>
    <button class="settings-button" onclick={() => goto(resolve('/settings'))}>
      <Icon name="settings" size={14} />
      {t('page.manageFeeds')}
    </button>
  </aside>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="resize-handle sidebar-resize-handle"
    role="separator"
    aria-label={t('page.resizeSidebar')}
    aria-orientation="vertical"
    aria-valuemin={160}
    aria-valuemax={clampSidebarWidth(360)}
    aria-valuenow={layoutSidebarWidth}
    tabindex="0"
    onpointerdown={beginReaderResize}
    onpointermove={resizeSidebar}
    onpointerup={endResize}
    onpointercancel={endResize}
    onlostpointercapture={endResize}
    onkeydown={resizeSidebarWithKey}
  ></div>
  <main class="pane posts-pane">
    {#key `${selectedFeedId}-${selectedFolderId}-${selectedToday}-${selectedStarred}`}
      <PostsFeed
        {items}
        {feeds}
        {folders}
        loading={initialLoading}
        {selectedFeedId}
        {selectedFolderId}
        {selectedToday}
        {selectedStarred}
        {selectedItemId}
        hasMore={hasMoreItems}
        {unreadOnly}
        unreadCount={viewUnreadCount}
        onSelect={selectPost}
        onToggleSaved={toggleSaved}
        onEndReached={loadMoreItems}
        onUnreadOnlyChange={setUnreadOnly}
        onMarkAllRead={markAllRead}
      />
    {/key}
  </main>
  {#if selectedItem}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="resize-handle"
      role="separator"
      aria-label={t('page.resizeReader')}
      aria-orientation="vertical"
      aria-valuemin={320}
      aria-valuemax={clampReaderWidth(760)}
      aria-valuenow={readerPaneWidth}
      tabindex="0"
      onpointerdown={beginReaderResize}
      onpointermove={resizeReader}
      onpointerup={endResize}
      onpointercancel={endResize}
      onlostpointercapture={endResize}
      onkeydown={resizeReaderWithKey}
    ></div>
    <main class="pane reader-pane">
      <PostReader
        item={selectedItem}
        {feeds}
        onClose={() => (selectedItemId = null)}
        onToggleSaved={toggleSaved}
        onAskAi={(quotedText) => {
          pendingQuote = quotedText;
          aiPanelOpen = true;
        }}
        aiOpen={showAi}
        onToggleAi={() => (aiPanelOpen = !aiPanelOpen)}
      />
    </main>
  {/if}
  {#if showAi}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="resize-handle ai-resize-handle"
      role="separator"
      aria-label={t('page.resizeAi')}
      aria-orientation="vertical"
      aria-valuemin={300}
      aria-valuemax={560}
      aria-valuenow={effectiveAiWidth}
      tabindex="0"
      onpointerdown={beginReaderResize}
      onpointermove={resizeAiPanel}
      onpointerup={endResize}
      onpointercancel={endResize}
      onlostpointercapture={endResize}
      onkeydown={resizeAiPanelWithKey}
    ></div>
    <aside class="pane ai-pane">
      {#if AiPanelComponent}
        <AiPanelComponent
          item={selectedItem}
          onCollapse={() => (aiPanelOpen = false)}
          {pendingQuote}
          onConsumedPendingQuote={() => (pendingQuote = null)}
        />
      {:else}
        <div class="empty-placeholder" role="status">
          <Spinner size={20} />
          <p>{t('page.loadingAi')}</p>
        </div>
      {/if}
    </aside>
  {/if}
</div>

<style>
  .layout {
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    grid-template-columns: 220px 6px 270px 6px 500px 6px 400px;
    height: 100vh;
    height: 100dvh;
    min-height: 0;
    width: 100%;
    overflow: hidden;
    background: var(--color-bg);
    transition: grid-template-columns var(--transition-layout);
  }

  .layout.no-transition {
    transition: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .layout {
      transition: none;
    }
  }

  .pane {
    min-width: 0;
    min-height: 0;
  }

  .feed-list-pane {
    display: flex;
    flex-direction: column;
    background: var(--color-bg-secondary);
  }

  .feed-list-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .settings-button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: calc(100% - 16px);
    margin: 8px;
    padding: 8px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-body);
    text-align: left;
    cursor: pointer;
  }

  .settings-button:hover {
    border-color: var(--color-border);
    background: var(--color-bg);
    color: var(--color-text);
  }

  .posts-pane {
    background: var(--color-bg);
  }

  .reader-pane {
    overflow: hidden;
    background: var(--color-bg);
    container-type: inline-size;
  }

  .ai-pane {
    overflow: hidden;
    grid-column: 7 / 8;
  }

  .ai-resize-handle {
    grid-column: 6 / 7;
  }

  .sidebar-resize-handle {
    grid-column: 2 / 3;
  }

  /* The 6px track stays grabbable but reads as a single 1px hairline on its
     left edge (the pane boundary); it thickens to an accent bar while
     hovered, focused or dragged. */
  .resize-handle {
    position: relative;
    z-index: 1;
    width: 6px;
    height: 100%;
    background: var(--color-bg);
    cursor: col-resize;
    touch-action: none;
  }

  .resize-handle::before {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: 1px;
    background: var(--color-border);
    content: '';
    transition:
      width var(--transition),
      background-color var(--transition);
  }

  .resize-handle:hover::before,
  .resize-handle:focus-visible::before,
  .layout.no-transition .resize-handle:active::before {
    width: 2px;
    background: var(--color-accent);
  }

  .resize-handle:focus-visible {
    outline: none;
  }
</style>
