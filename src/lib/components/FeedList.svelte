<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';
  import Icon from '$lib/icons/Icon.svelte';
  import IconButton from './IconButton.svelte';
  import Spinner from './Spinner.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import { showToast } from '$lib/toast.svelte.ts';
  import { clickOutside } from '$lib/clickOutside';
  import { feedDisplayName, feedHasError } from '$lib/format';
  import { t } from '$lib/i18n/index.svelte';
  import type { FeedFolder, FeedSummary, SidebarScope } from '$lib/types';

  interface Props {
    feeds: FeedSummary[];
    folders: FeedFolder[];
    loading?: boolean;
    selectedFeedId: number | null;
    selectedFolderId: number | null;
    selectedToday?: boolean;
    selectedStarred?: boolean;
    /** Unread-only mode: hide feeds/folders with nothing unread (§3.16). */
    unreadOnly?: boolean;
    /** Sidebar filter text: hides feeds/folders whose name doesn't match. */
    filter?: string;
    onSelectFeed: (feedId: number | null) => void;
    onSelectFolder: (folderId: number | null) => void;
    onSelectToday?: () => void;
    onSelectStarred?: () => void;
    onMoveFeed: (feedId: number, folderId: number | null) => Promise<void>;
    /** Reorders feeds within one folder (or the unfiled group, `null`); §J. */
    onReorderFeeds?: (folderId: number | null, feedIds: number[]) => Promise<void>;
    onTogglePinned?: (feedId: number, pinned: boolean) => Promise<void>;
    onMarkRead?: (scope: SidebarScope) => void;
    onRefreshFeed?: (feedId: number) => void;
    onOpenSettings?: (scope?: SidebarScope) => void;
  }

  let {
    feeds,
    folders,
    loading = false,
    selectedFeedId,
    selectedFolderId,
    selectedToday = false,
    selectedStarred = false,
    unreadOnly = false,
    filter = '',
    onSelectFeed,
    onSelectFolder,
    onSelectToday = () => {},
    onSelectStarred = () => {},
    onMoveFeed,
    onReorderFeeds,
    onTogglePinned,
    onMarkRead,
    onRefreshFeed,
    onOpenSettings,
  }: Props = $props();

  const COLLAPSED_KEY = 'aequo:collapsedFolders';

  const visibleFeeds = $derived(feeds.filter((feed) => !feed.hiddenFromAll));
  const totalUnread = $derived(visibleFeeds.reduce((sum, feed) => sum + feed.unreadCount, 0));
  const unfiledFeeds = $derived(feeds.filter((feed) => feed.folderId === null));

  // Memoized once per `feeds` change instead of re-filtering/re-reducing the
  // full feed list once per folder on every render.
  const feedsByFolder = $derived.by(() => {
    const byFolder: Record<number, FeedSummary[]> = {};
    for (const feed of feeds) {
      if (feed.folderId === null) continue;
      (byFolder[feed.folderId] ??= []).push(feed);
    }
    return byFolder;
  });
  const unreadByFolder = $derived.by(() => {
    const byFolder: Record<number, number> = {};
    for (const [folderId, list] of Object.entries(feedsByFolder)) {
      byFolder[Number(folderId)] = list.reduce((sum, feed) => sum + feed.unreadCount, 0);
    }
    return byFolder;
  });

  const normalizedFilter = $derived(filter.trim().toLowerCase());
  function matchesFilter(feed: FeedSummary): boolean {
    return !normalizedFilter || feedName(feed).toLowerCase().includes(normalizedFilter);
  }
  function folderMatchesFilter(folderId: number): boolean {
    if (!normalizedFilter) return true;
    const folder = folders.find((f) => f.id === folderId);
    if (folder?.name.toLowerCase().includes(normalizedFilter)) return true;
    return (feedsByFolder[folderId] ?? []).some(matchesFilter);
  }

  // In unread-only mode a feed/folder with nothing unread is hidden - except
  // the current selection, so finishing its last post doesn't yank it out
  // of the sidebar; it drops out once the user moves to another view.
  function showFeed(feed: FeedSummary): boolean {
    return (
      (!unreadOnly || feed.unreadCount > 0 || feed.id === selectedFeedId) && matchesFilter(feed)
    );
  }
  function showFolder(folderId: number): boolean {
    return (
      (!unreadOnly ||
        (unreadByFolder[folderId] ?? 0) > 0 ||
        folderId === selectedFolderId ||
        (feedsByFolder[folderId] ?? []).some((feed) => feed.id === selectedFeedId)) &&
      folderMatchesFilter(folderId)
    );
  }
  const shownFolders = $derived(folders.filter((folder) => showFolder(folder.id)));
  const shownUnfiledFeeds = $derived(unfiledFeeds.filter(showFeed));
  const allCaughtUp = $derived(
    unreadOnly && shownFolders.length === 0 && shownUnfiledFeeds.length === 0,
  );
  const noFilterMatches = $derived(
    !!normalizedFilter && shownFolders.length === 0 && shownUnfiledFeeds.length === 0,
  );
  // Shown above the folder list regardless of unread-only/caught-up/filter
  // state below, as its own always-reachable shortcut (§K).
  const pinnedFeeds = $derived(
    feeds.filter((feed) => feed.pinned && !feed.hiddenFromAll).filter(showFeed),
  );

  function feedName(feed: FeedSummary): string {
    return feedDisplayName(feed);
  }

  // Collapsed folders survive restarts; ids of deleted folders are simply
  // never rendered, so stale entries are harmless.
  function readCollapsed(): number[] {
    try {
      const parsed = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]');
      return Array.isArray(parsed) ? parsed.filter((id) => Number.isInteger(id)) : [];
    } catch {
      return [];
    }
  }
  const collapsedFolders = new SvelteSet<number>(readCollapsed());

  const PINNED_COLLAPSED_KEY = 'aequo:pinnedCollapsed';
  let pinnedCollapsed = $state(
    (() => {
      try {
        return localStorage.getItem(PINNED_COLLAPSED_KEY) === 'true';
      } catch {
        return false;
      }
    })(),
  );
  function togglePinnedCollapsed() {
    pinnedCollapsed = !pinnedCollapsed;
    try {
      localStorage.setItem(PINNED_COLLAPSED_KEY, String(pinnedCollapsed));
    } catch {
      // Still collapsed for this session.
    }
  }

  function toggleCollapsed(folderId: number) {
    if (collapsedFolders.has(folderId)) collapsedFolders.delete(folderId);
    else collapsedFolders.add(folderId);
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...collapsedFolders]));
    } catch {
      // Still collapsed for this session.
    }
  }

  // --- Drag and drop between folders -------------------------------------

  let dragOverTarget = $state<number | 'unfiled' | null>(null);
  let draggingFeedId = $state<number | null>(null);

  function handleDragStart(event: DragEvent, feedId: number) {
    event.dataTransfer?.setData('text/plain', String(feedId));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
    draggingFeedId = feedId;
  }

  function handleDragOver(event: DragEvent, target: number | 'unfiled') {
    event.preventDefault();
    dragOverTarget = target;
  }

  function handleDragLeave(target: number | 'unfiled') {
    if (dragOverTarget === target) dragOverTarget = null;
  }

  async function moveFeedTo(feedId: number, folderId: number | null) {
    try {
      await onMoveFeed(feedId, folderId);
    } catch (error) {
      showToast(t('sidebar.moveFailed', { error: String(error) }), { type: 'error' });
    }
  }

  function handleDrop(event: DragEvent, folderId: number | null) {
    event.preventDefault();
    dragOverTarget = null;
    const raw = event.dataTransfer?.getData('text/plain');
    const feedId = raw ? Number(raw) : NaN;
    if (!Number.isNaN(feedId)) moveFeedTo(feedId, folderId);
  }

  // --- Drag and drop within a folder (§J) ---------------------------------

  let dragOverFeedId = $state<number | null>(null);

  function siblingIds(folderId: number | null): number[] {
    return (folderId === null ? unfiledFeeds : (feedsByFolder[folderId] ?? [])).map((f) => f.id);
  }

  async function reorderFeeds(folderId: number | null, feedIds: number[]) {
    try {
      await onReorderFeeds?.(folderId, feedIds);
    } catch (error) {
      showToast(t('sidebar.moveFailed', { error: String(error) }), { type: 'error' });
    }
  }

  function handleFeedDragOver(event: DragEvent, feedId: number) {
    event.preventDefault();
    event.stopPropagation();
    if (draggingFeedId !== feedId) dragOverFeedId = feedId;
  }

  function handleFeedDragLeave(feedId: number) {
    if (dragOverFeedId === feedId) dragOverFeedId = null;
  }

  /** Dropping onto another feed's own row reorders within that feed's
   * folder; dropping onto a different folder's feed still moves it there,
   * same as dropping on that folder's header. */
  function handleFeedDrop(event: DragEvent, target: FeedSummary) {
    event.preventDefault();
    event.stopPropagation();
    dragOverFeedId = null;
    const raw = event.dataTransfer?.getData('text/plain');
    const feedId = raw ? Number(raw) : NaN;
    if (Number.isNaN(feedId) || feedId === target.id) return;
    const dragged = feeds.find((f) => f.id === feedId);
    if (!dragged) return;
    if (dragged.folderId !== target.folderId) {
      moveFeedTo(feedId, target.folderId);
      return;
    }
    const ids = siblingIds(target.folderId).filter((id) => id !== feedId);
    ids.splice(ids.indexOf(target.id), 0, feedId);
    void reorderFeeds(target.folderId, ids);
  }

  // --- Row menu: the same actions from `⋯` and from right-click ----------

  type MenuTarget = { kind: 'feed'; feed: FeedSummary } | { kind: 'folder'; folder: FeedFolder };
  let menu = $state<(MenuTarget & { x: number; y: number; returnFocus: HTMLElement }) | null>(null);

  const MENU_WIDTH = 210;
  const MENU_MAX_HEIGHT = 320;

  function openMenu(target: MenuTarget, x: number, y: number, returnFocus: HTMLElement) {
    // Keep the fixed-position menu inside the window.
    menu = {
      ...target,
      x: Math.min(x, window.innerWidth - MENU_WIDTH - 8),
      y: Math.min(y, window.innerHeight - MENU_MAX_HEIGHT - 8),
      returnFocus,
    };
  }

  function openMenuFromButton(target: MenuTarget, event: MouseEvent) {
    const trigger = event.currentTarget as HTMLElement;
    if (menu && menu.returnFocus === trigger) {
      closeMenu();
      return;
    }
    const rect = trigger.getBoundingClientRect();
    openMenu(target, rect.left, rect.bottom + 4, trigger);
  }

  function openContextMenu(target: MenuTarget, event: MouseEvent) {
    event.preventDefault();
    openMenu(target, event.clientX, event.clientY, event.currentTarget as HTMLElement);
  }

  function closeMenu(restoreFocus = false) {
    const focusTarget = menu?.returnFocus;
    menu = null;
    if (restoreFocus) focusTarget?.focus();
  }

  // Run before closing: the action reads the menu's target, which closing clears.
  function runMenuAction(action: () => void) {
    action();
    closeMenu();
  }

  function focusFirstItem(node: HTMLElement) {
    node.querySelector<HTMLElement>('[role="menuitem"]:not(:disabled)')?.focus();
  }

  function handleMenuKeydown(event: KeyboardEvent) {
    const items = [
      ...(event.currentTarget as HTMLElement).querySelectorAll<HTMLElement>(
        '[role="menuitem"]:not(:disabled)',
      ),
    ];
    const index = items.indexOf(document.activeElement as HTMLElement);
    if (event.key === 'Escape') {
      event.preventDefault();
      closeMenu(true);
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      items[(index + 1) % items.length]?.focus();
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      items[(index - 1 + items.length) % items.length]?.focus();
    } else if (event.key === 'Tab') {
      closeMenu();
    }
  }

  function feedErrorTooltip(feed: FeedSummary): string {
    if (feed.lastError) return t('sidebar.feedError', { error: feed.lastError });
    return t('sidebar.feedFailures', { count: feed.consecutiveFailures });
  }

  const allSelected = $derived(
    selectedFeedId === null && selectedFolderId === null && !selectedToday && !selectedStarred,
  );
</script>

{#snippet feedRow(feed: FeedSummary, nested: boolean)}
  <div
    class="item-row"
    class:nested
    class:drag-over={dragOverFeedId === feed.id}
    role="presentation"
    ondragover={(event) => handleFeedDragOver(event, feed.id)}
    ondragleave={() => handleFeedDragLeave(feed.id)}
    ondrop={(event) => handleFeedDrop(event, feed)}
  >
    <button
      class="row"
      class:selected={selectedFeedId === feed.id}
      class:quiet={feed.unreadCount === 0}
      class:dragging={draggingFeedId === feed.id}
      aria-current={selectedFeedId === feed.id ? 'true' : undefined}
      title={feedHasError(feed)
        ? `${feed.title ?? feed.url}\n${feedErrorTooltip(feed)}`
        : (feed.title ?? feed.url)}
      draggable="true"
      ondragstart={(event) => handleDragStart(event, feed.id)}
      ondragend={() => (draggingFeedId = null)}
      onclick={() => onSelectFeed(feed.id)}
      oncontextmenu={(event) => openContextMenu({ kind: 'feed', feed }, event)}
    >
      <FeedIcon {feed} />
      <span class="title">{feedName(feed)}</span>
      {#if feedHasError(feed)}
        <span class="feed-health" role="img" aria-label={feedErrorTooltip(feed)}
          ><Icon name="error" size={12} /></span
        >
      {/if}
      {#if feed.unreadCount > 0}<span class="count">{feed.unreadCount}</span>{/if}
    </button>
    <IconButton
      class="more"
      size="sm"
      iconSize={14}
      icon="more"
      label={t('sidebar.actionsFor', { name: feedName(feed) })}
      ariaHaspopup="menu"
      ariaExpanded={menu?.kind === 'feed' && menu.feed.id === feed.id}
      onclick={(event) => openMenuFromButton({ kind: 'feed', feed }, event)}
    />
  </div>
{/snippet}

<nav class="feed-list" aria-label={t('sidebar.nav')}>
  <h2 class="visually-hidden">{t('sidebar.heading')}</h2>
  <button
    class="row fixed"
    class:selected={allSelected}
    aria-current={allSelected ? 'true' : undefined}
    onclick={() => onSelectFeed(null)}
  >
    <span class="row-icon"><Icon name="inbox" size={15} /></span>
    <span class="title">{t('sidebar.all')}</span>
    {#if totalUnread > 0}<span class="count">{totalUnread}</span>{/if}
  </button>
  <button
    class="row fixed"
    class:selected={selectedToday}
    aria-current={selectedToday ? 'true' : undefined}
    onclick={onSelectToday}
  >
    <span class="row-icon"><Icon name="clock" size={15} /></span>
    <span class="title">{t('sidebar.today')}</span>
  </button>
  <button
    class="row fixed"
    class:selected={selectedStarred}
    aria-current={selectedStarred ? 'true' : undefined}
    onclick={onSelectStarred}
  >
    <span class="row-icon"><Icon name="star-outline" size={15} /></span>
    <span class="title">{t('sidebar.starred')}</span>
  </button>

  {#if pinnedFeeds.length > 0}
    <!-- "Pinned" is a filtered view over feed.pinned, not a real foldable
         entity, so unlike a folder it has no ⋯ menu (nothing sensible to put
         in it - no rename/delete/bulk-unpin exists) and no drag target
         (pinning already has one clear affordance: the per-feed ⋯ menu's
         pin/unpin toggle). Intentional asymmetry, not an oversight
         (docs/PANEL_DESIGN_UNIFICATION_PLAN.md §4.1-C). -->
    <div class="folder-group" role="group" aria-label={t('sidebar.pinned')}>
      <div class="item-row folder-row">
        <button
          class="row-icon-button chevron"
          class:collapsed={pinnedCollapsed}
          type="button"
          aria-expanded={!pinnedCollapsed}
          aria-label={t(pinnedCollapsed ? 'sidebar.expand' : 'sidebar.collapse', {
            name: t('sidebar.pinned'),
          })}
          onclick={togglePinnedCollapsed}
          ><Icon name="chevron-down" size={12} /></button
        >
        <span class="row-icon"><Icon name="star" size={13} /></span>
        <span class="title">{t('sidebar.pinned')}</span>
      </div>
      {#if !pinnedCollapsed}
        {#each pinnedFeeds as feed (feed.id)}
          {@render feedRow(feed, true)}
        {/each}
      {/if}
    </div>
  {/if}

  {#if loading && feeds.length === 0 && folders.length === 0}
    <div class="empty-placeholder">
      <Spinner size={20} />
    </div>
  {:else if feeds.length === 0 && folders.length === 0}
    <div class="empty-placeholder">
      <span class="empty-placeholder-icon"><Icon name="empty" size={20} /></span>
      <h3>{t('sidebar.noFeedsTitle')}</h3>
      <p>{t('sidebar.noFeedsBody')}</p>
    </div>
  {:else if allCaughtUp}
    <p class="caught-up">{t('sidebar.caughtUp')}</p>
  {:else if noFilterMatches}
    <p class="caught-up">{t('sidebar.noFilterMatches')}</p>
  {:else}
    <div class="groups">
      {#each shownFolders as folder (folder.id)}
        {@const folderFeeds = feedsByFolder[folder.id] ?? []}
        {@const folderUnread = unreadByFolder[folder.id] ?? 0}
        {@const isCollapsed = normalizedFilter ? false : collapsedFolders.has(folder.id)}
        <div
          class="folder-group"
          role="group"
          aria-label={t('sidebar.folderGroup', { name: folder.name })}
        >
          <div
            class="item-row folder-row"
            class:drag-over={dragOverTarget === folder.id}
            role="presentation"
            ondragover={(event) => handleDragOver(event, folder.id)}
            ondragleave={() => handleDragLeave(folder.id)}
            ondrop={(event) => handleDrop(event, folder.id)}
          >
            <button
              class="row-icon-button chevron"
              class:collapsed={isCollapsed}
              type="button"
              aria-expanded={!isCollapsed}
              aria-label={t(isCollapsed ? 'sidebar.expand' : 'sidebar.collapse', {
                name: folder.name,
              })}
              onclick={() => toggleCollapsed(folder.id)}
              ><Icon name="chevron-down" size={12} /></button
            >
            <button
              class="row folder"
              class:selected={selectedFolderId === folder.id}
              class:quiet={folderUnread === 0}
              aria-current={selectedFolderId === folder.id ? 'true' : undefined}
              title={folder.name}
              type="button"
              onclick={() => onSelectFolder(folder.id)}
              oncontextmenu={(event) => openContextMenu({ kind: 'folder', folder }, event)}
            >
              <span class="title">{folder.name}</span>
              {#if folderUnread > 0}<span class="count">{folderUnread}</span>{/if}
            </button>
            <IconButton
              class="more"
              size="sm"
              iconSize={14}
              icon="more"
              label={t('sidebar.actionsFor', { name: folder.name })}
              ariaHaspopup="menu"
              ariaExpanded={menu?.kind === 'folder' && menu.folder.id === folder.id}
              onclick={(event) => openMenuFromButton({ kind: 'folder', folder }, event)}
            />
          </div>
          {#if !isCollapsed}
            {#if folderFeeds.length === 0}
              <p class="empty-folder-hint">{t('sidebar.dropHere')}</p>
            {:else}
              {#each folderFeeds.filter(showFeed) as feed (feed.id)}
                {@render feedRow(feed, true)}
              {/each}
            {/if}
          {/if}
        </div>
      {/each}

      {#if shownUnfiledFeeds.length > 0 || (folders.length > 0 && !unreadOnly)}
        <div
          class="unfiled-group"
          class:drag-over={dragOverTarget === 'unfiled'}
          role="group"
          aria-label={t('sidebar.unfiledGroup')}
          ondragover={(event) => handleDragOver(event, 'unfiled')}
          ondragleave={() => handleDragLeave('unfiled')}
          ondrop={(event) => handleDrop(event, null)}
        >
          {#if folders.length > 0}
            <p class="group-label">{t('sidebar.unfiled')}</p>
          {/if}
          {#each shownUnfiledFeeds as feed (feed.id)}
            {@render feedRow(feed, false)}
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</nav>

{#if menu}
  {@const current = menu}
  <div
    class="menu ui-popup"
    role="menu"
    tabindex="-1"
    style:left="{current.x}px"
    style:top="{current.y}px"
    aria-label={t('sidebar.actionsFor', {
      name: current.kind === 'feed' ? feedName(current.feed) : current.folder.name,
    })}
    use:focusFirstItem
    use:clickOutside={() => closeMenu()}
    onkeydown={handleMenuKeydown}
  >
    {#if current.kind === 'feed'}
      {@const feed = current.feed}
      {#if feedHasError(feed)}
        <p class="menu-error">{feedErrorTooltip(feed)}</p>
      {/if}
      <button
        type="button"
        role="menuitem"
        disabled={feed.unreadCount === 0}
        onclick={() => runMenuAction(() => onMarkRead?.({ feedId: feed.id }))}
        ><Icon name="check-all" size={14} />{t('sidebar.markRead')}</button
      >
      <button
        type="button"
        role="menuitem"
        onclick={() => runMenuAction(() => onRefreshFeed?.(feed.id))}
        ><Icon name="retry" size={14} />{t('sidebar.refresh')}</button
      >
      <button
        type="button"
        role="menuitem"
        onclick={() => runMenuAction(() => onTogglePinned?.(feed.id, !feed.pinned))}
        ><Icon name={feed.pinned ? 'star' : 'star-outline'} size={14} />{t(
          feed.pinned ? 'sidebar.unpin' : 'sidebar.pin',
        )}</button
      >
      {#if folders.length > 0}
        <div class="menu-separator" role="separator"></div>
        <p class="menu-label">{t('sidebar.moveTo')}</p>
        {#if feed.folderId !== null}
          <button
            type="button"
            role="menuitem"
            class="indented"
            onclick={() => runMenuAction(() => moveFeedTo(feed.id, null))}
            >{t('sidebar.unfiled')}</button
          >
        {/if}
        {#each folders.filter((folder) => folder.id !== feed.folderId) as target (target.id)}
          <button
            type="button"
            role="menuitem"
            class="indented"
            onclick={() => runMenuAction(() => moveFeedTo(feed.id, target.id))}
            >{target.name}</button
          >
        {/each}
      {/if}
      <div class="menu-separator" role="separator"></div>
      <button
        type="button"
        role="menuitem"
        onclick={() => runMenuAction(() => onOpenSettings?.({ feedId: feed.id }))}
        ><Icon name="settings" size={14} />{t('sidebar.feedSettings')}</button
      >
    {:else}
      {@const folder = current.folder}
      <button
        type="button"
        role="menuitem"
        disabled={(unreadByFolder[folder.id] ?? 0) === 0}
        onclick={() => runMenuAction(() => onMarkRead?.({ folderId: folder.id }))}
        ><Icon name="check-all" size={14} />{t('sidebar.markRead')}</button
      >
      <div class="menu-separator" role="separator"></div>
      <button
        type="button"
        role="menuitem"
        onclick={() => runMenuAction(() => onOpenSettings?.({ folderId: folder.id }))}
        ><Icon name="settings" size={14} />{t('sidebar.folderSettings')}</button
      >
    {/if}
  </div>
{/if}

<style>
  .feed-list {
    min-height: 100%;
    padding: var(--space-2) var(--space-2) var(--space-4);
  }

  /* One row grammar for fixed views, folders and feeds: 32px tall, a 16px
     icon column, name, right-aligned count. */
  .row {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    min-height: 32px;
    padding: 6px var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--color-hover-bg);
  }
  .row.selected {
    background: var(--color-selection-bg);
  }
  .row.selected .title {
    font-weight: var(--weight-medium);
  }
  .row[draggable='true'] {
    -webkit-user-drag: element;
    user-select: none;
    -webkit-user-select: none;
  }
  .row.dragging {
    opacity: 0.5;
  }
  .row.folder .title {
    font-weight: var(--weight-medium);
  }
  /* Nothing unread: stays listed (so the sidebar doesn't reshuffle) but
     recedes, letting feeds with something to read stand out. */
  .row.quiet:not(.selected) .title {
    color: var(--color-text-secondary);
  }

  .row-icon {
    display: inline-flex;
    width: 16px;
    flex-shrink: 0;
    justify-content: center;
    color: var(--color-text-secondary);
  }

  .title {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    flex-shrink: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
  }

  .item-row {
    position: relative;
    display: flex;
    align-items: center;
  }
  .item-row.nested {
    padding-left: var(--space-5);
  }

  .row-icon-button {
    display: grid;
    width: 24px;
    height: 28px;
    flex: 0 0 24px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
  }
  .row-icon-button:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  /* `⋯` keeps its width (no row shift) and only shows on hover, keyboard
     focus within the row, or while its menu is open; without a hover-capable
     pointer it stays visible. `.more` is now IconButton's own root element
     (passed through via its `class` prop), so it needs :global() to match
     past IconButton's own Svelte scoping - scoped under the (locally-scoped)
     `.item-row` ancestor so this doesn't leak to an unrelated `.more`
     elsewhere in the app. */
  .item-row :global(.more) {
    opacity: 0;
  }
  .item-row:hover :global(.more),
  .item-row:focus-within :global(.more),
  .item-row :global(.more[aria-expanded='true']) {
    opacity: 1;
  }
  @media (hover: none) {
    .item-row :global(.more) {
      opacity: 1;
    }
  }

  .feed-health {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--color-error);
  }

  .chevron :global(svg) {
    transition: transform var(--transition);
  }
  .chevron.collapsed :global(svg) {
    transform: rotate(-90deg);
  }

  .groups {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }

  .folder-row .chevron {
    flex-basis: 16px;
    width: 16px;
  }

  .group-label {
    margin: var(--space-2) var(--space-2) 2px;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    font-weight: var(--weight-medium);
  }

  .empty-folder-hint {
    margin: 2px 0 6px var(--space-5);
    padding-left: var(--space-2);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .caught-up {
    margin: var(--space-4) var(--space-3);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .folder-row.drag-over,
  .unfiled-group.drag-over {
    border-radius: var(--radius-sm);
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
    background: var(--color-selection-bg);
  }

  .item-row.drag-over {
    border-radius: var(--radius-sm);
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
  }

  .menu {
    position: fixed;
    z-index: 60;
    display: flex;
    width: 210px;
    max-height: 320px;
    flex-direction: column;
    overflow-y: auto;
  }
  .menu button {
    min-height: var(--control-size);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    border: 0;
    border-radius: var(--control-radius);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-small);
    text-align: left;
    cursor: pointer;
  }
  .menu button :global(svg) {
    flex-shrink: 0;
    color: var(--color-text-secondary);
  }
  .menu button:hover:not(:disabled),
  .menu button:focus-visible {
    background: var(--color-hover-bg);
    outline: none;
  }
  .menu button:disabled {
    color: var(--color-text-tertiary);
    cursor: default;
  }
  .menu button.indented {
    padding-left: calc(var(--space-2) + 14px + var(--space-2));
  }
  .menu-separator {
    height: 1px;
    margin: var(--space-1) 0;
    background: var(--color-border);
  }
  .menu-error {
    margin: 2px var(--space-2) var(--space-1);
    color: var(--color-error);
    font-size: var(--text-small);
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
  .menu-label {
    margin: 2px var(--space-2);
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
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
