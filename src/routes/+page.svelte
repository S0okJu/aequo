<script lang="ts">
  import { aiAutoOpen } from '$lib/ai/preferences.svelte';
  import { onMount, tick, untrack } from 'svelte';
  import { fly } from 'svelte/transition';
  import { beforeNavigate, goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { listen } from '@tauri-apps/api/event';
  import type AiPanelType from '$lib/components/AiPanel.svelte';
  import AddFeedDialog from '$lib/components/AddFeedDialog.svelte';
  import FeedList from '$lib/components/FeedList.svelte';
  import PostsFeed from '$lib/components/PostsFeed.svelte';
  import PostReader from '$lib/components/PostReader.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import Spinner from '$lib/components/Spinner.svelte';
  import { showToast } from '$lib/toast.svelte.ts';
  import { t } from '$lib/i18n/index.svelte';
  import { feedHasError } from '$lib/format';
  import { rise } from '$lib/motion';
  import { TimelineWindow } from '$lib/timeline.svelte';
  import { resourceSettings } from '$lib/resources.svelte';
  import { serializedBytes } from '$lib/cacheBudget';
  import {
    saveReadingCheckpoint,
    takeReadingCheckpoint,
    type ListPosition,
  } from '$lib/readingCheckpoint';
  import {
    listAiSessions,
    listFeeds,
    listFeedFolders,
    listItems,
    getItemSummary,
    countUnreadItems,
    markItemsRead,
    markItemsUnread,
    markRead,
    refreshFeed,
    refreshFeedBatch,
    reorderFeedsInFolder,
    setFeedPinned,
    setItemSaved,
    updateFeedOrganization,
  } from '$lib/api';
  import type {
    FeedFolder,
    FeedSummary,
    Item,
    ItemFilter,
    SidebarScope,
    AiContextRequest,
  } from '$lib/types';

  const REFRESH_DEBOUNCE_MS = 200;
  const AI_PANEL_OPEN_KEY = 'aequo:aiPanelOpen';
  const AI_STANDALONE_KEY = 'aequo:aiStandalone';
  const SIDEBAR_WIDTH_KEY = 'aequo:sidebarWidth';
  const UNREAD_ONLY_KEY = 'aequo:unreadOnly';
  const resumed = takeReadingCheckpoint();
  let listPosition: ListPosition | null = null;
  let resumeListPosition = $state<ListPosition | null>(null);
  let sidebarScrollEl: HTMLDivElement | undefined;
  let readerScrollTop = resumed?.readerScrollTop ?? 0;

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

  function readStoredAiStandalone(): boolean {
    try {
      return localStorage.getItem(AI_STANDALONE_KEY) === 'true';
    } catch {
      return false;
    }
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
    reconciled?: boolean;
  }

  let feeds = $state<FeedSummary[]>([]);
  let folders = $state<FeedFolder[]>([]);
  const timeline = new TimelineWindow(listItems);
  const items = $derived(timeline.items);
  let initialLoading = $state(true);
  let scopeUnread = $state<{ key: string; count: number; sampledAt: number } | null>(null);
  let scopeCountRequest = 0;
  let scopeCountInFlight: { key: string; request: number; promise: Promise<void> } | null = null;
  let pageAlive = true;
  const hasMoreItems = $derived(timeline.hasOlder);
  let selectedFeedId = $state<number | null>(resumed?.feedId ?? null);
  let selectedFolderId = $state<number | null>(resumed?.folderId ?? null);
  let selectedToday = $state(resumed?.today ?? false);
  let selectedStarred = $state(resumed?.starred ?? false);
  let selectedItemId = $state<number | null>(resumed?.selectedItemId ?? null);
  let selectedItemRecord = $state<Item | null>(null);
  let unreadOnly = $state(resumed?.unreadOnly ?? readStoredUnreadOnly());
  // Search within the current view (§3.14); cleared when the view changes.
  let searchQuery = $state(resumed?.query ?? '');
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let refreshingView = $state(false);
  // When the current view's first page was loaded (unix seconds): "mark all
  // read" only touches items fetched by then, never ones that arrived since.
  let viewLoadedAt = 0;
  let viewportWidth = $state(initialViewportWidth());
  let sidebarWidth = $state(readStoredSidebarWidth());
  let readerWidth = $state(resumed?.readerWidth ?? 500);
  let aiWidth = $state(resumed?.aiWidth ?? 400);
  let aiPanelOpen = $state(readStoredAiPanelOpen());
  let aiContextRequest = $state<AiContextRequest | null>(null);
  let aiPinned = $state(false);
  let aiStandalone = $state(readStoredAiStandalone());
  let aiBusy = $state(false);
  let contextRequestKey = 0;
  let aiPaneElement = $state<HTMLElement | undefined>(undefined);

  async function openContextItem(id: number) {
    try {
      const post = await getItemSummary(id);
      if (post) {
        selectedItemRecord = post;
        selectedItemId = id;
      }
    } catch (error) {
      showToast(String(error), { type: 'error' });
    }
  }
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
  // The post `aiHasHistory` describes, so a late reply for a post the user
  // already left can't light up the dot for the one now open.
  let aiHistoryItemId = $state<number | null>(null);
  let aiHasHistory = $state(false);
  let aiHistoryRevision = $state(0);
  let aiHistoryGeneration = 0;
  const postListWidth = $derived(Math.min(270, Math.max(220, Math.round(viewportWidth * 0.2))));

  const selectedItem = $derived(
    items.find((item) => item.id === selectedItemId) ??
      (selectedItemRecord?.id === selectedItemId ? selectedItemRecord : null),
  );

  $effect(() => {
    const id = selectedItemId;
    const current = items.find((item) => item.id === id);
    untrack(() => {
      if (id === null) selectedItemRecord = null;
      else if (current) selectedItemRecord = current;
    });
  });

  $effect(() => {
    const pages = resourceSettings().retainedPages;
    untrack(() => {
      timeline.setPageLimit(pages);
      timeline.account();
    });
  });

  $effect(() => {
    // Metadata edits (read/save/undo) change the serialized cache size too.
    void serializedBytes(items);
    untrack(() => timeline.account());
  });

  // Every grid track below is a plain pixel number (never a mix of fixed
  // and `minmax(...,1fr)` shapes) so `.layout`'s grid-template-columns
  // transition can interpolate smoothly instead of snapping - browsers
  // can't animate between differently-shaped track definitions.
  const showReader = $derived(selectedItem !== null);
  // Main-screen conversations do not require an open article.
  const showAi = $derived(
    aiPanelOpen && (aiStandalone || showReader || aiContextRequest !== null || aiPinned),
  );
  const aiDrawer = $derived(
    showAi && showReader && viewportWidth - sidebarWidth - postListWidth - aiWidth - 18 < 320,
  );
  const aiTrack = $derived(showAi && !aiDrawer);
  const readerHandleWidth = $derived(showReader ? 6 : 0);
  const aiHandleWidth = $derived(aiTrack ? 6 : 0);
  const minimumReaderWidth = $derived(showReader ? 320 : 0);
  const minimumAiWidth = $derived(aiTrack ? 300 : 0);
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
    aiTrack
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
      localStorage.setItem(AI_STANDALONE_KEY, String(aiStandalone));
    } catch {
      /* The mode still applies while this page is open. */
    }
  });

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

  // Whether the open post has saved conversations, for the reader's AI dot.
  // Asked here rather than in the panel because the dot's whole job is to
  // show while the panel is closed (and so not even loaded).
  $effect(() => {
    void aiHistoryRevision;
    const generation = ++aiHistoryGeneration;
    const itemId = selectedItem?.id ?? null;
    aiHistoryItemId = itemId;
    aiHasHistory = false;
    if (itemId === null) return;
    Promise.all([listAiSessions(itemId, 'codex'), listAiSessions(itemId, 'claude')])
      .then(([codex, claude]) => {
        if (aiHistoryItemId === itemId && generation === aiHistoryGeneration)
          aiHasHistory = codex.length + claude.length > 0;
      })
      .catch(() => {
        // A missing dot is a better failure than a visible error here.
      });
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
    if (!pageAlive) return;
    feeds = nextFeeds;
    folders = nextFolders;
    const valid = new Set(nextFeeds.map((feed) => feed.id));
    timeline.retainFeeds(valid);
    if (selectedItem && !valid.has(selectedItem.feedId)) selectedItemId = null;
    await refreshScopeUnread();
  }

  // The filter for whatever the sidebar has selected. Starred ignores the
  // unread-only toggle: saved posts are kept on purpose, read or not.
  function currentFilter(): ItemFilter {
    const filter: ItemFilter = { unreadOnly: unreadOnly && !selectedStarred };
    if (searchQuery.trim()) filter.query = searchQuery.trim();
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

  async function refreshScopeUnread(force = false): Promise<void> {
    if (!pageAlive || (!selectedToday && !selectedStarred && !searchQuery.trim())) return;
    const filter = currentFilter();
    const key = JSON.stringify(filter);
    if (
      !force &&
      scopeCountInFlight?.key === key &&
      scopeCountInFlight.request === scopeCountRequest
    )
      return scopeCountInFlight.promise;
    if (!force && scopeUnread?.key === key && Date.now() - scopeUnread.sampledAt < 200) return;
    const request = ++scopeCountRequest;
    const promise: Promise<void> = countUnreadItems(filter)
      .then((count) => {
        if (request === scopeCountRequest && key === JSON.stringify(currentFilter())) {
          scopeUnread = { key, count, sampledAt: Date.now() };
        }
      })
      .catch((error) => console.error('count_unread_items failed:', error))
      .finally(() => {
        if (scopeCountInFlight?.promise === promise) scopeCountInFlight = null;
      });
    scopeCountInFlight = { key, request, promise };
    return promise;
  }

  // First page of the merged timeline (§6.3) for the current selection -
  // replaces items entirely (a feed-updated event or a selection change can
  // shift what "page 1" even means). A background refresh in unread-only
  // mode keeps posts the user already read in place, so a post doesn't
  // vanish from under them mid-read; they drop out on the next view change.
  async function refreshItems(background = false) {
    const loadedAt = Math.floor(Date.now() / 1000);
    if (await timeline.refresh(currentFilter(), background)) viewLoadedAt = loadedAt;
    await refreshScopeUnread();
  }

  function setSearchQuery(query: string) {
    searchQuery = query;
    timeline.invalidate();
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => refreshItems(), 200);
  }

  // Feeds the current view draws from; Today and Starred span them all.
  function feedsInView(): FeedSummary[] {
    if (selectedFeedId !== null) return feeds.filter((feed) => feed.id === selectedFeedId);
    if (selectedFolderId !== null)
      return feeds.filter((feed) => feed.folderId === selectedFolderId);
    return feeds.filter((feed) => selectedStarred || selectedToday || !feed.hiddenFromAll);
  }

  // New posts land through the usual feed-updated events; a failure on one
  // feed doesn't stop the others.
  async function refreshView() {
    if (refreshingView) return;
    refreshingView = true;
    try {
      const { failed } = await refreshFeedBatch(feedsInView().map((feed) => feed.id));
      if (failed > 0) showToast(t('toast.refreshSomeFailed', { count: failed }), { type: 'error' });
      await Promise.all([refreshFeeds(), refreshItems()]);
    } finally {
      refreshingView = false;
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
  // every page, not just the loaded ones). Filtered views use a DB count too.
  const viewUnreadCount = $derived.by(() => {
    if (selectedToday || selectedStarred || searchQuery.trim()) {
      return scopeUnread?.key === JSON.stringify(currentFilter()) ? scopeUnread.count : null;
    }
    const inView = feeds.filter((feed) =>
      selectedFeedId !== null
        ? feed.id === selectedFeedId
        : selectedFolderId !== null
          ? feed.folderId === selectedFolderId
          : !feed.hiddenFromAll,
    );
    return inView.reduce((sum, feed) => sum + feed.unreadCount, 0);
  });

  // "Mark all read" for the open view (only what was loaded by then) or for
  // a feed/folder from the sidebar menu (everything fetched so far).
  function markAllRead() {
    return markFilterRead({ ...currentFilter(), unreadOnly: false }, viewLoadedAt);
  }

  function markScopeRead(scope: SidebarScope) {
    return markFilterRead(scope, Math.floor(Date.now() / 1000));
  }

  async function markFilterRead(filter: ItemFilter, fetchedBefore: number) {
    let ids: number[];
    try {
      ids = await markItemsRead(filter, fetchedBefore);
    } catch (error) {
      showToast(t('toast.markAllReadFailed'), { type: 'error' });
      console.error('mark_items_read failed:', error);
      return;
    }
    if (ids.length === 0) return;
    const marked = new Set(ids);
    for (const item of items) if (marked.has(item.id)) item.isRead = true;
    if (selectedItem && marked.has(selectedItem.id)) selectedItem.isRead = true;
    scopeUnread = null;
    scopeCountRequest++;
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
    if (selectedItem && restored.has(selectedItem.id)) selectedItem.isRead = false;
    scopeUnread = null;
    scopeCountRequest++;
    await refreshFeeds();
  }

  // The post feed requests another page as the reader nears the bottom.
  async function loadMoreItems() {
    await timeline.more('older');
  }

  async function loadNewerItems() {
    await timeline.more('newer');
  }

  function selectFeed(feedId: number | null) {
    if (
      selectedFeedId === feedId &&
      selectedFolderId === null &&
      !selectedToday &&
      !selectedStarred
    )
      return;
    selectedFeedId = feedId;
    selectedFolderId = null;
    selectedToday = false;
    selectedStarred = false;
    selectedItemId = null;
    searchQuery = '';
    refreshItems();
  }

  function selectFolder(folderId: number | null) {
    if (
      selectedFolderId === folderId &&
      selectedFeedId === null &&
      !selectedToday &&
      !selectedStarred
    )
      return;
    selectedFeedId = null;
    selectedFolderId = folderId;
    selectedToday = false;
    selectedStarred = false;
    selectedItemId = null;
    searchQuery = '';
    refreshItems();
  }

  function selectToday() {
    if (selectedToday) return;
    selectedFeedId = null;
    selectedFolderId = null;
    selectedToday = true;
    selectedStarred = false;
    selectedItemId = null;
    searchQuery = '';
    refreshItems();
  }

  function selectStarred() {
    if (selectedStarred) return;
    selectedFeedId = null;
    selectedFolderId = null;
    selectedToday = false;
    selectedStarred = true;
    selectedItemId = null;
    searchQuery = '';
    refreshItems();
  }

  async function moveFeed(feedId: number, folderId: number | null) {
    const feed = feeds.find((f) => f.id === feedId);
    if (!feed || feed.folderId === folderId) return;
    await updateFeedOrganization(feedId, folderId, feed.hiddenFromAll);
    await refreshFeeds();
  }

  async function reorderFeeds(folderId: number | null, feedIds: number[]) {
    await reorderFeedsInFolder(folderId, feedIds);
    await refreshFeeds();
  }

  async function toggleFeedPinned(feedId: number, pinned: boolean) {
    await setFeedPinned(feedId, pinned);
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
    } finally {
      void refreshScopeUnread(true);
    }
  }

  function selectPost(itemId: number) {
    if (!aiStandalone && !aiAutoOpen() && selectedItemId !== itemId) aiPanelOpen = false;
    readerScrollTop = 0;
    selectedItemRecord = items.find((item) => item.id === itemId) ?? null;
    selectedItemId = selectedItemId === itemId ? null : itemId;
    markPostRead(itemId);
  }

  // Feed and folder settings live on the settings page; the sidebar links
  // straight to the subscriptions section, with the feed's details open.
  function openSettings(scope?: SidebarScope) {
    if (!scope) goto(resolve('/settings'));
    else if ('feedId' in scope)
      goto(resolve(`/settings?section=subscriptions&feed=${scope.feedId}`));
    else goto(resolve('/settings?section=subscriptions'));
  }

  let addFeedOpen = $state(false);
  // New feeds land in the folder being viewed (or the viewed feed's folder).
  const addFeedDefaultFolderId = $derived(
    selectedFolderId ?? feeds.find((feed) => feed.id === selectedFeedId)?.folderId ?? null,
  );

  const failingFeedCount = $derived(feeds.filter(feedHasError).length);

  function addFeedFromSidebar() {
    addFeedOpen = true;
  }

  async function handleFeedAdded(feedId: number) {
    addFeedOpen = false;
    await refreshFeeds();
    selectFeed(feedId);
    showToast(t('toast.feedAdded'), { type: 'success' });
  }

  // Refresh results arrive through the usual `feed-updated` event.
  async function refreshOneFeed(feedId: number) {
    try {
      await refreshFeed(feedId);
    } catch (error) {
      const feed = feeds.find((entry) => entry.id === feedId);
      showToast(t('toast.refreshFailed', { name: feed?.title ?? feed?.url ?? '' }), {
        type: 'error',
      });
      console.error('refresh_feed failed:', error);
    }
  }

  async function markPostUnread(itemId: number) {
    const item =
      items.find((entry) => entry.id === itemId) ??
      (selectedItem?.id === itemId ? selectedItem : null);
    if (!item || !item.isRead) return;
    item.isRead = false;
    const feed = feeds.find((entry) => entry.id === item.feedId);
    if (feed) feed.unreadCount += 1;
    try {
      await markItemsUnread([itemId]);
    } catch (error) {
      item.isRead = true;
      if (feed) feed.unreadCount = Math.max(0, feed.unreadCount - 1);
      showToast(t('toast.markUnreadFailed'), { type: 'error' });
      console.error('mark_items_unread failed:', error);
    } finally {
      void refreshScopeUnread(true);
    }
  }

  async function toggleSaved(itemId: number) {
    const item =
      items.find((entry) => entry.id === itemId) ??
      (selectedItem?.id === itemId ? selectedItem : null);
    if (!item) return;
    const wasVisible = items.some((entry) => entry.id === itemId);
    const filter = currentFilter();
    const filterKey = JSON.stringify(filter);
    const previous = item.isSaved;
    item.isSaved = !previous;
    // Unstarring while viewing the Starred tab should drop the item from
    // view immediately, same as how it'd vanish on the next refresh.
    if (selectedStarred && !item.isSaved) {
      timeline.remove(itemId);
    }
    try {
      await setItemSaved(itemId, item.isSaved);
      if (
        selectedStarred &&
        item.isSaved &&
        !wasVisible &&
        filterKey === JSON.stringify(currentFilter())
      ) {
        try {
          // Seek immediately before this row. It is first iff the shared DB
          // filter (including full-text search) includes it; no ghost insertion.
          const match = await listItems(filter, 1, {
            publishedAt: item.publishedAt,
            id: item.id + 1,
          });
          if (
            item.isSaved &&
            filterKey === JSON.stringify(currentFilter()) &&
            match[0]?.id === itemId
          )
            timeline.restore(match[0]);
        } catch (error) {
          console.error('saved view membership check failed:', error);
        }
      }
    } catch (error) {
      item.isSaved = previous;
      if (
        selectedStarred &&
        previous &&
        wasVisible &&
        filterKey === JSON.stringify(currentFilter())
      ) {
        timeline.restore(item);
      }
      showToast(t('toast.saveFailed'), { type: 'error' });
      console.error('set_item_saved failed:', error);
    } finally {
      void refreshScopeUnread(true);
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
  let pendingReconciliation = false;
  function scheduleBackgroundRefresh(event?: { payload?: FeedUpdatedPayload }) {
    pendingReconciliation ||= event?.payload?.reconciled === true;
    clearTimeout(scheduledRefreshTimer);
    scheduledRefreshTimer = setTimeout(() => {
      if (!refreshFeedsInFlight) {
        refreshFeedsInFlight = true;
        refreshFeeds().finally(() => (refreshFeedsInFlight = false));
      }
      if (!refreshItemsInFlight) {
        refreshItemsInFlight = true;
        const reconciled = pendingReconciliation;
        pendingReconciliation = false;
        const selectedId = selectedItemId;
        if (reconciled && selectedId !== null) {
          void getItemSummary(selectedId)
            .then((item) => {
              if (selectedItemId === selectedId) selectedItemRecord = item;
            })
            .catch(() => {});
        }
        refreshItems(!reconciled).finally(() => {
          refreshItemsInFlight = false;
          if (pendingReconciliation) scheduleBackgroundRefresh();
        });
      }
    }, REFRESH_DEBOUNCE_MS);
  }

  beforeNavigate(({ to }) => {
    if (!to?.url.pathname.startsWith('/settings')) return;
    const anchor =
      items.find((item) => item.id === (listPosition?.id ?? timeline.anchorId)) ?? items[0];
    saveReadingCheckpoint({
      feedId: selectedFeedId,
      folderId: selectedFolderId,
      today: selectedToday,
      starred: selectedStarred,
      unreadOnly,
      query: searchQuery,
      selectedItemId,
      list: anchor
        ? {
            id: anchor.id,
            publishedAt: anchor.publishedAt,
            offset: listPosition?.id === anchor.id ? listPosition.offset : 0,
          }
        : null,
      readerScrollTop,
      sidebarScrollTop: sidebarScrollEl?.scrollTop ?? 0,
      readerWidth,
      aiWidth,
    });
  });

  async function loadReadingView() {
    if (!resumed) {
      await Promise.all([refreshFeeds(), refreshItems()]);
      return;
    }
    await refreshFeeds();
    if (!pageAlive) return;
    const priorFilter = JSON.stringify(currentFilter());
    if (selectedFeedId !== null && !feeds.some((feed) => feed.id === selectedFeedId))
      selectedFeedId = null;
    if (selectedFolderId !== null && !folders.some((folder) => folder.id === selectedFolderId))
      selectedFolderId = null;
    const filter = currentFilter();
    const filterKey = JSON.stringify(filter);
    const summary =
      resumed.selectedItemId === null
        ? Promise.resolve(null)
        : getItemSummary(resumed.selectedItemId).catch((error) => {
            console.error('get_item_summary failed:', error);
            if (pageAlive) showToast(t('reader.restoreFailed'), { type: 'error' });
            return null;
          });
    const [, selected] = await Promise.all([
      resumed.list && priorFilter === filterKey
        ? timeline.resume(filter, resumed.list)
        : timeline.refresh(filter),
      summary,
    ]);
    if (!pageAlive || filterKey !== JSON.stringify(currentFilter())) return;
    if (selectedItemId === resumed.selectedItemId) {
      selectedItemRecord = selected;
      selectedItemId = selected?.id ?? null;
    }
    viewLoadedAt = Math.floor(Date.now() / 1000);
    const anchor =
      items.find((item) => item.id === resumed.list?.id) ?? items[Math.min(49, items.length - 1)];
    if (anchor && priorFilter === filterKey)
      resumeListPosition = {
        id: anchor.id,
        offset: anchor.id === resumed.list?.id ? resumed.list.offset : 0,
      };
    await refreshScopeUnread(true);
    await tick();
    if (pageAlive && sidebarScrollEl) sidebarScrollEl.scrollTop = resumed.sidebarScrollTop;
  }

  onMount(() => {
    const updateViewport = () => {
      viewportWidth = document.documentElement.clientWidth || window.innerWidth;
    };
    updateViewport();
    window.addEventListener('resize', updateViewport);
    const closeDrawer = (event: PointerEvent) => {
      if (
        aiDrawer &&
        !aiPaneElement?.contains(event.target as Node) &&
        !(event.target as Element)?.closest?.('.reader-ai-toggle')
      )
        aiPanelOpen = false;
    };
    const escapeDrawer = (event: KeyboardEvent) => {
      if (aiDrawer && event.key === 'Escape') aiPanelOpen = false;
    };
    // Global open/close for the AI panel (§H) - mirrors the sidebar "AI Chat"
    // entry point's own open behavior so it's reachable the same way from
    // anywhere, not just when a reader/context already makes it visible.
    const toggleAiShortcut = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key !== '/') return;
      event.preventDefault();
      if (aiPanelOpen) {
        aiPanelOpen = false;
      } else {
        aiStandalone = true;
        aiPanelOpen = true;
      }
    };
    document.addEventListener('pointerdown', closeDrawer);
    window.addEventListener('keydown', escapeDrawer);
    window.addEventListener('keydown', toggleAiShortcut);
    void loadReadingView()
      .catch((error) => {
        console.error('load reading view failed:', error);
        if (pageAlive) timeline.error = String(error);
      })
      .finally(() => {
        if (pageAlive) initialLoading = false;
      });

    // Event-driven only (§3.2/§6.2) - no polling timer anywhere here.
    const unlisten = listen<FeedUpdatedPayload>('feed-updated', scheduleBackgroundRefresh);

    return () => {
      window.removeEventListener('resize', updateViewport);
      document.removeEventListener('pointerdown', closeDrawer);
      window.removeEventListener('keydown', escapeDrawer);
      window.removeEventListener('keydown', toggleAiShortcut);
      clearTimeout(scheduledRefreshTimer);
      clearTimeout(searchTimer);
      timeline.dispose();
      pageAlive = false;
      scopeCountRequest++;
      void unlisten.then((fn) => fn()).catch(() => undefined);
    };
  });
</script>

<div
  class="layout"
  class:no-transition={isDragging}
  style={`grid-template-columns: ${layoutSidebarWidth}px 6px ${postListComputedWidth}px ${readerHandleWidth}px ${readerPaneWidth}px ${aiHandleWidth}px ${effectiveAiWidth}px`}
>
  <aside class="pane feed-list-pane">
    <div class="sidebar-top">
      <button
        class="ai-entry ui-action"
        class:active={showAi && aiStandalone}
        type="button"
        aria-pressed={showAi && aiStandalone}
        onclick={() => {
          aiStandalone = true;
          aiPanelOpen = true;
        }}><Icon name="ask-ai" size={15} /><span>{t('sidebar.aiChat')}</span></button
      >
    </div>
    <div class="feed-list-scroll" bind:this={sidebarScrollEl}>
      <FeedList
        {feeds}
        {folders}
        loading={initialLoading || timeline.loading}
        {selectedFeedId}
        {selectedFolderId}
        {selectedToday}
        {selectedStarred}
        {unreadOnly}
        onSelectFeed={selectFeed}
        onSelectFolder={selectFolder}
        onSelectToday={selectToday}
        onSelectStarred={selectStarred}
        onMoveFeed={moveFeed}
        onReorderFeeds={reorderFeeds}
        onTogglePinned={toggleFeedPinned}
        onMarkRead={markScopeRead}
        onRefreshFeed={refreshOneFeed}
        onOpenSettings={openSettings}
      />
    </div>
    <footer class="sidebar-footer">
      {#if failingFeedCount > 0}
        <button
          class="feed-errors"
          type="button"
          title={t('sidebar.settings')}
          transition:fly={rise()}
          onclick={() => goto(resolve('/settings?section=subscriptions'))}
        >
          <span class="status-dot" aria-hidden="true"></span>
          <span class="feed-errors-text"
            >{t('sidebar.feedErrors', { count: failingFeedCount })}</span
          >
          <Icon name="chevron-right" size={12} />
        </button>
      {/if}
      <div class="footer-actions">
        <button class="footer-button" type="button" onclick={addFeedFromSidebar}>
          <Icon name="plus" size={15} />
          <span>{t('sidebar.addFeed')}</span>
        </button>
        <button class="footer-button" type="button" onclick={() => openSettings()}>
          <Icon name="settings" size={15} />
          <span>{t('sidebar.settings')}</span>
        </button>
      </div>
    </footer>
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
        hasNewer={timeline.hasNewer}
        loadingPage={timeline.loadingPage}
        loadError={timeline.error}
        viewKey={`${JSON.stringify(currentFilter())}:${timeline.resetToken}`}
        onRetry={() => refreshItems()}
        onStartReached={loadNewerItems}
        resumePosition={resumeListPosition}
        onAnchorChange={(id, offset) => {
          timeline.anchorId = id;
          listPosition = { id, offset };
        }}
        {unreadOnly}
        unreadCount={viewUnreadCount}
        onSelect={selectPost}
        onToggleSaved={toggleSaved}
        onEndReached={loadMoreItems}
        onUnreadOnlyChange={setUnreadOnly}
        onMarkAllRead={markAllRead}
        query={searchQuery}
        onQueryChange={setSearchQuery}
        onRefresh={refreshView}
        refreshing={refreshingView}
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
        initialScrollTop={resumed?.selectedItemId === selectedItemId ? resumed.readerScrollTop : 0}
        onScrollPositionChange={(top) => (readerScrollTop = top)}
        {feeds}
        onClose={() => (selectedItemId = null)}
        onToggleSaved={toggleSaved}
        onAskAi={(quotedText) => {
          pendingQuote = quotedText;
          aiPanelOpen = true;
        }}
        onMarkUnread={markPostUnread}
        aiOpen={showAi}
        aiHistory={aiHasHistory}
        onToggleAi={() => (aiPanelOpen = !aiPanelOpen)}
        {aiBusy}
        onSummarize={() => {
          if (!selectedItem || aiBusy) return;
          aiContextRequest = {
            key: ++contextRequestKey,
            items: [selectedItem],
            action: 'summarize',
          };
          aiPanelOpen = true;
        }}
      />
    </main>
  {/if}
  {#if aiTrack}
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
  {/if}
  {#if AiPanelComponent || showAi}
    <aside
      class="pane ai-pane"
      class:ai-drawer={aiDrawer}
      class:ai-hidden={!showAi}
      bind:this={aiPaneElement}
    >
      {#if AiPanelComponent}
        <AiPanelComponent
          item={selectedItem}
          standalone={aiStandalone}
          {feeds}
          contextRequest={aiContextRequest}
          onPinnedChange={(value) => (aiPinned = value)}
          onBusyChange={(value) => (aiBusy = value)}
          onHistoryChange={() => aiHistoryRevision++}
          onOpenItem={openContextItem}
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

<AddFeedDialog
  open={addFeedOpen}
  {folders}
  defaultFolderId={addFeedDefaultFolderId}
  onAdded={handleFeedAdded}
  onClose={() => (addFeedOpen = false)}
/>

<style>
  .ai-hidden {
    display: none;
  }
  .ai-pane.ai-drawer {
    position: fixed;
    z-index: 30;
    right: 0;
    top: 0;
    bottom: 0;
    width: min(400px, 100vw);
    background: var(--color-bg);
    box-shadow: -6px 0 20px color-mix(in srgb, var(--color-text) 12%, transparent);
    animation: drawer-in var(--motion-layout) var(--ease-enter);
  }
  @keyframes drawer-in {
    from {
      transform: translateX(100%);
    }
  }

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

  /* 48px row for the whole-service AI entry, matching PostsFeed's .bar /
     PostReader's .reader-action-bar convention
     (docs/PANEL_DESIGN_UNIFICATION_PLAN.md §5 step 4). */
  .sidebar-top {
    display: flex;
    min-height: 48px;
    flex-shrink: 0;
    align-items: center;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  /* Pinned below the scrolling list: a quiet status line when something
     needs attention, then two equal actions that always say what they do. */
  .sidebar-footer {
    display: flex;
    flex-shrink: 0;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2);
    border-top: 1px solid var(--color-border);
  }

  .footer-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-1);
  }

  .footer-button {
    display: flex;
    min-width: 0;
    min-height: var(--control-size);
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-body);
    cursor: pointer;
  }

  /* Promoted above the feed list rather than grouped with the "Add feed" /
     "Settings" utilities below: this is the primary entry point into the
     whole-service AI chat, not a housekeeping action - but built on the
     same shared .ui-action pill every other "Ask AI" entry point in the
     app already uses (PostsFeed's "Ask about this view", PostReader's
     "Ask AI"/"Summarize"), so it reads as the same button language as
     the search toggle beside it instead of a one-off accent-colored
     control (docs/PANEL_DESIGN_UNIFICATION_PLAN.md). Only layout and the
     active/pressed look are overridden here; color, hover, focus, sizing
     and padding come from .ui-action. */
  .ai-entry {
    min-width: 0;
    flex: 1;
    justify-content: flex-start;
    color: var(--color-text);
    text-align: left;
  }

  .ai-entry span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ai-entry.active {
    background: var(--color-selection-bg);
  }

  .footer-button span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .footer-button :global(svg) {
    flex-shrink: 0;
  }

  .footer-button:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  .feed-errors {
    display: flex;
    min-height: 28px;
    align-items: center;
    gap: var(--space-2);
    padding: 4px var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    text-align: left;
    cursor: pointer;
  }

  .feed-errors:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  .status-dot {
    width: 7px;
    height: 7px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--color-error);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-error) 18%, transparent);
  }

  .feed-errors-text {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    container-type: inline-size;
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

  /* Always a 2px box, pinned to the left edge by `transform-origin: left` -
     scaleX(0.5) reads as the resting 1px hairline, scaleX(1) as the 2px
     accent bar. transform (not width) is what actually animates, so
     hovering/focusing never triggers layout (docs/UNIFIED_UX_MOTION_PLAN.md
     §8.1 forbids animating width/height). */
  .resize-handle::before {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: 2px;
    background: var(--color-border);
    content: '';
    transform: scaleX(0.5);
    transform-origin: left;
    transition:
      transform var(--transition),
      background-color var(--transition);
  }

  .resize-handle:hover::before,
  .resize-handle:focus-visible::before,
  .layout.no-transition .resize-handle:active::before {
    background: var(--color-accent);
    transform: scaleX(1);
  }

  .resize-handle:focus-visible {
    outline: none;
  }
</style>
