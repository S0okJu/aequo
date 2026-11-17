<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';
  import Icon from '$lib/icons/Icon.svelte';
  import Spinner from './Spinner.svelte';
  import { showToast } from '$lib/toast.svelte.ts';
  import type { FeedFolder, FeedSummary } from '$lib/types';

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
    onSelectFeed: (feedId: number | null) => void;
    onSelectFolder: (folderId: number | null) => void;
    onSelectToday?: () => void;
    onSelectStarred?: () => void;
    onManageFolders: () => void;
    onMoveFeed: (feedId: number, folderId: number | null) => Promise<void>;
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
    onSelectFeed,
    onSelectFolder,
    onSelectToday = () => {},
    onSelectStarred = () => {},
    onManageFolders,
    onMoveFeed,
  }: Props = $props();

  const visibleFeeds = $derived(feeds.filter((feed) => !feed.hiddenFromAll));
  const totalUnread = $derived(visibleFeeds.reduce((sum, feed) => sum + feed.unreadCount, 0));
  const unfiledFeeds = $derived(feeds.filter((feed) => feed.folderId === null));

  // Memoized once per `feeds` change instead of re-filtering/re-reducing the
  // full feed list once per folder on every render. Plain objects (not Map)
  // since folder ids are always numbers and these are immutable snapshots,
  // not reactive state that needs SvelteMap's own tracking.
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

  // In unread-only mode a feed/folder with nothing unread is hidden - except
  // the current selection, so finishing its last post doesn't yank it out
  // of the sidebar; it drops out once the user moves to another view.
  function showFeed(feed: FeedSummary): boolean {
    return !unreadOnly || feed.unreadCount > 0 || feed.id === selectedFeedId;
  }
  function showFolder(folderId: number): boolean {
    return (
      !unreadOnly ||
      (unreadByFolder[folderId] ?? 0) > 0 ||
      folderId === selectedFolderId ||
      (feedsByFolder[folderId] ?? []).some((feed) => feed.id === selectedFeedId)
    );
  }
  const shownFolders = $derived(folders.filter((folder) => showFolder(folder.id)));
  const shownUnfiledFeeds = $derived(unfiledFeeds.filter(showFeed));
  const allCaughtUp = $derived(
    unreadOnly && shownFolders.length === 0 && shownUnfiledFeeds.length === 0,
  );

  let collapsedFolders = new SvelteSet<number>();
  let dragOverTarget = $state<number | 'unfiled' | null>(null);
  let draggingFeedId = $state<number | null>(null);
  let openMoveMenuFeedId = $state<number | null>(null);
  let moveMenuDropUp = $state(false);

  function toggleCollapsed(folderId: number) {
    if (collapsedFolders.has(folderId)) collapsedFolders.delete(folderId);
    else collapsedFolders.add(folderId);
  }

  function handleDragStart(event: DragEvent, feedId: number) {
    event.dataTransfer?.setData('text/plain', String(feedId));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
    draggingFeedId = feedId;
  }

  function handleDragEnd() {
    draggingFeedId = null;
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
      showToast(`Couldn't move feed: ${String(error)}`, { type: 'error' });
    }
  }

  function handleDrop(event: DragEvent, folderId: number | null) {
    event.preventDefault();
    dragOverTarget = null;
    const raw = event.dataTransfer?.getData('text/plain');
    const feedId = raw ? Number(raw) : NaN;
    if (!Number.isNaN(feedId)) moveFeedTo(feedId, folderId);
  }

  function toggleMoveMenu(feedId: number, event: MouseEvent) {
    if (openMoveMenuFeedId === feedId) {
      closeMoveMenu();
      return;
    }
    openMoveMenuFeedId = feedId;
    const trigger = event.currentTarget as HTMLElement;
    const rect = trigger.getBoundingClientRect();
    moveMenuDropUp = window.innerHeight - rect.bottom < 160;
  }

  function closeMoveMenu() {
    openMoveMenuFeedId = null;
  }

  function selectMoveTarget(feed: FeedSummary, folderId: number | null) {
    closeMoveMenu();
    moveFeedTo(feed.id, folderId);
  }

  function focusFirstMenuItem(node: HTMLElement) {
    node.querySelector<HTMLElement>('.move-menu-item')?.focus();
  }

  function clickOutside(node: HTMLElement, onOutside: () => void) {
    function handle(event: MouseEvent) {
      if (!node.contains(event.target as Node)) onOutside();
    }
    document.addEventListener('mousedown', handle, true);
    return {
      destroy() {
        document.removeEventListener('mousedown', handle, true);
      },
    };
  }

  function feedHasError(feed: FeedSummary): boolean {
    return feed.lastError !== null || feed.consecutiveFailures > 0;
  }

  function feedErrorTooltip(feed: FeedSummary): string {
    if (feed.lastError) return `Last refresh failed: ${feed.lastError}`;
    return `${feed.consecutiveFailures} consecutive failed refresh${feed.consecutiveFailures === 1 ? '' : 'es'}`;
  }
</script>

<nav class="feed-list" aria-label="Feed navigation">
  <button
    class="row"
    class:selected={selectedFeedId === null && selectedFolderId === null && !selectedToday && !selectedStarred}
    onclick={() => onSelectFeed(null)}
  >
    <span class="title">All</span>
    {#if totalUnread > 0}<span class="count">{totalUnread}</span>{/if}
  </button>
  <button
    class="row"
    class:selected={selectedFeedId === null && selectedFolderId === null && selectedToday}
    onclick={onSelectToday}
  >
    <span class="row-icon"><Icon name="clock" size={14} /></span><span class="title">Today</span>
  </button>
  <button
    class="row"
    class:selected={selectedFeedId === null && selectedFolderId === null && selectedStarred}
    onclick={onSelectStarred}
  >
    <span class="row-icon"><Icon name="star" size={14} /></span><span class="title">Starred</span>
  </button>
  {#if loading && feeds.length === 0 && folders.length === 0}
    <div class="empty-placeholder">
      <Spinner size={20} />
    </div>
  {:else if feeds.length === 0 && folders.length === 0}
    <div class="empty-placeholder">
      <span class="empty-placeholder-icon"><Icon name="empty" size={20} /></span>
      <h3>No feeds yet</h3>
      <p>Use "Manage feeds" below to add your first subscription.</p>
    </div>
  {:else}
    <div class="section-label">
      <span>FOLDERS</span>
      <button
        type="button"
        aria-label="Manage folders"
        title="Manage folders"
        onclick={onManageFolders}>Manage →</button
      >
    </div>

    {#each shownFolders as folder (folder.id)}
      {@const folderFeeds = feedsByFolder[folder.id] ?? []}
      {@const folderUnread = unreadByFolder[folder.id] ?? 0}
      {@const isCollapsed = collapsedFolders.has(folder.id)}
      <div class="folder-group">
        <div
          class="folder-row"
          role="group"
          aria-label={`${folder.name} folder`}
          class:selected={selectedFolderId === folder.id}
          class:drag-over={dragOverTarget === folder.id}
          ondragover={(event) => handleDragOver(event, folder.id)}
          ondragleave={() => handleDragLeave(folder.id)}
          ondrop={(event) => handleDrop(event, folder.id)}
        >
          <button
            class="folder-collapse"
            type="button"
            aria-label={isCollapsed ? `Expand ${folder.name}` : `Collapse ${folder.name}`}
            onclick={() => toggleCollapsed(folder.id)}
          >
            <span class="chevron" class:collapsed={isCollapsed}><Icon name="chevron-down" size={12} /></span>
          </button>
          <button class="folder-select" type="button" onclick={() => onSelectFolder(folder.id)}>
            <span class="title">{folder.name}</span>
            {#if folderUnread > 0}<span class="count">{folderUnread}</span>{/if}
          </button>
        </div>
        {#if !isCollapsed}
          <div class="folder-children">
            {#if folderFeeds.length === 0}
              <p class="empty-folder-hint">Drop feeds here</p>
            {:else}
              {#each folderFeeds.filter(showFeed) as feed (feed.id)}
                <div class="feed-row">
                  <button
                    class="row nested"
                    class:selected={selectedFeedId === feed.id}
                    class:quiet={feed.unreadCount === 0}
                    class:dragging={draggingFeedId === feed.id}
                    draggable="true"
                    ondragstart={(event) => handleDragStart(event, feed.id)}
                    ondragend={handleDragEnd}
                    onclick={() => onSelectFeed(feed.id)}
                  >
                    <span class="title">{feed.title ?? feed.url}</span>
                    {#if feedHasError(feed)}<span
                        class="feed-health"
                        role="img"
                        aria-label={feedErrorTooltip(feed)}
                        title={feedErrorTooltip(feed)}
                      ><Icon name="error" size={11} /></span>{/if}
                    {#if feed.unreadCount > 0}<span class="count">{feed.unreadCount}</span>{/if}
                  </button>
                  <button
                    type="button"
                    class="feed-move-trigger"
                    id={`move-trigger-${feed.id}`}
                    aria-label={`Move ${feed.title ?? feed.url}`}
                    title="Move to folder"
                    aria-haspopup="menu"
                    aria-expanded={openMoveMenuFeedId === feed.id}
                    onclick={(event) => toggleMoveMenu(feed.id, event)}><Icon name="more" size={14} /></button
                  >
                  {#if openMoveMenuFeedId === feed.id}
                    <div
                      class="move-menu"
                      class:drop-up={moveMenuDropUp}
                      role="menu"
                      tabindex="-1"
                      aria-label={`Move ${feed.title ?? feed.url} to folder`}
                      use:focusFirstMenuItem
                      use:clickOutside={closeMoveMenu}
                      onkeydown={(event) => {
                        if (event.key === 'Escape') {
                          event.preventDefault();
                          closeMoveMenu();
                          document.getElementById(`move-trigger-${feed.id}`)?.focus();
                        }
                      }}
                    >
                      <button
                        type="button"
                        role="menuitem"
                        class="move-menu-item"
                        onclick={() => selectMoveTarget(feed, null)}>Unfiled</button
                      >
                      {#each folders.filter((f) => f.id !== feed.folderId) as target (target.id)}
                        <button
                          type="button"
                          role="menuitem"
                          class="move-menu-item"
                          onclick={() => selectMoveTarget(feed, target.id)}>{target.name}</button
                        >
                      {/each}
                    </div>
                  {/if}
                </div>
              {/each}
            {/if}
          </div>
        {/if}
      </div>
    {/each}

    {#if allCaughtUp}
      <p class="caught-up">All caught up</p>
    {:else if shownUnfiledFeeds.length > 0 || (folders.length > 0 && !unreadOnly)}
      <div
        class="unfiled-group"
        role="group"
        aria-label="Unfiled feeds"
        class:drag-over={dragOverTarget === 'unfiled'}
        ondragover={(event) => handleDragOver(event, 'unfiled')}
        ondragleave={() => handleDragLeave('unfiled')}
        ondrop={(event) => handleDrop(event, null)}
      >
        <div class="section-label unfiled-label">UNFILED</div>
        {#each shownUnfiledFeeds as feed (feed.id)}
          <div class="feed-row">
            <button
              class="row"
              class:selected={selectedFeedId === feed.id}
              class:quiet={feed.unreadCount === 0}
              class:dragging={draggingFeedId === feed.id}
              draggable="true"
              ondragstart={(event) => handleDragStart(event, feed.id)}
              ondragend={handleDragEnd}
              onclick={() => onSelectFeed(feed.id)}
            >
              <span class="title">{feed.title ?? feed.url}</span>
              {#if feedHasError(feed)}<span
                  class="feed-health"
                  role="img"
                  aria-label={feedErrorTooltip(feed)}
                  title={feedErrorTooltip(feed)}
                ><Icon name="error" size={11} /></span>{/if}
              {#if feed.unreadCount > 0}<span class="count">{feed.unreadCount}</span>{/if}
            </button>
            {#if folders.length > 0}
              <button
                type="button"
                class="feed-move-trigger"
                id={`move-trigger-${feed.id}`}
                aria-label={`Move ${feed.title ?? feed.url}`}
                title="Move to folder"
                aria-haspopup="menu"
                aria-expanded={openMoveMenuFeedId === feed.id}
                onclick={(event) => toggleMoveMenu(feed.id, event)}><Icon name="more" size={14} /></button
              >
              {#if openMoveMenuFeedId === feed.id}
                <div
                  class="move-menu"
                  class:drop-up={moveMenuDropUp}
                  role="menu"
                  tabindex="-1"
                  aria-label={`Move ${feed.title ?? feed.url} to folder`}
                  use:focusFirstMenuItem
                  use:clickOutside={closeMoveMenu}
                  onkeydown={(event) => {
                    if (event.key === 'Escape') {
                      event.preventDefault();
                      closeMoveMenu();
                      document.getElementById(`move-trigger-${feed.id}`)?.focus();
                    }
                  }}
                >
                  {#each folders as target (target.id)}
                    <button
                      type="button"
                      role="menuitem"
                      class="move-menu-item"
                      onclick={() => selectMoveTarget(feed, target.id)}>{target.name}</button
                    >
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</nav>

<style>
  .feed-list {
    min-height: 100%;
    padding: 8px 4px 14px;
  }
  .row,
  .folder-select {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 30px;
    padding: 7px 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    text-align: left;
    cursor: pointer;
  }
  .row:hover,
  .folder-select:hover {
    background: var(--color-hover-bg);
  }
  .row[draggable='true'] {
    cursor: grab;
    -webkit-user-drag: element;
    user-select: none;
    -webkit-user-select: none;
  }
  .row[draggable='true']:active {
    cursor: grabbing;
  }
  .row.dragging {
    opacity: 0.5;
  }
  .feed-row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .feed-row .row {
    flex: 1;
    min-width: 0;
  }
  .empty-folder-hint {
    margin: 2px 5px 6px 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    font-style: italic;
    line-height: 1.4;
  }
  .feed-move-trigger {
    display: grid;
    width: 22px;
    height: 22px;
    flex: 0 0 22px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    font: inherit;
    cursor: pointer;
    opacity: 0;
  }
  .feed-move-trigger {
    opacity: 0.55;
  }
  .feed-row:hover .feed-move-trigger,
  .feed-move-trigger:focus-visible,
  .feed-move-trigger[aria-expanded='true'] {
    opacity: 1;
  }
  .feed-move-trigger:hover {
    background: var(--color-hover-bg);
  }
  .move-menu {
    position: absolute;
    top: 100%;
    right: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    gap: 1px;
    max-width: 140px;
    margin-top: 2px;
    padding: 4px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.18);
  }
  .move-menu.drop-up {
    top: auto;
    bottom: 100%;
    margin-top: 0;
    margin-bottom: 2px;
  }
  .move-menu-item {
    display: block;
    width: 100%;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .move-menu-item:hover,
  .move-menu-item:focus-visible {
    background: var(--color-hover-bg);
  }
  .row.selected,
  .folder-row.selected {
    background: var(--color-selection-bg);
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
    margin-left: auto;
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    background: var(--color-hover-bg);
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
    font-weight: var(--weight-medium);
    line-height: 1.4;
  }
  .row.selected .count,
  .folder-row.selected .count {
    background: var(--color-bg);
  }
  .feed-health {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    margin: 0 1px;
    color: var(--color-error);
  }
  .row-icon {
    display: inline-flex;
    width: 14px;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    color: var(--color-text-tertiary);
  }
  .section-label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 18px 5px 5px 9px;
    padding-top: 14px;
    border-top: 1px solid var(--color-border);
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    font-weight: var(--weight-bold);
    letter-spacing: 0.08em;
  }
  .section-label button {
    flex-shrink: 0;
    border: 0;
    background: transparent;
    color: var(--color-text-tertiary);
    font: inherit;
    font-size: var(--text-caption);
    font-weight: var(--weight-bold);
    letter-spacing: normal;
    text-transform: none;
    cursor: pointer;
  }
  .section-label button:hover {
    color: var(--color-accent);
  }
  .folder-row {
    display: flex;
    align-items: center;
    border-radius: var(--radius-sm);
    transition:
      background-color var(--transition),
      outline-color var(--transition);
  }
  .folder-select {
    flex: 1;
    font-weight: var(--weight-medium);
  }
  .folder-collapse {
    display: grid;
    width: 20px;
    height: 20px;
    flex: 0 0 20px;
    place-items: center;
    margin-left: 2px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
  }
  .chevron {
    display: inline-block;
    transition: transform var(--transition);
  }
  .chevron.collapsed {
    transform: rotate(-90deg);
  }
  .nested {
    font-size: var(--text-body);
  }
  .folder-children {
    margin: 2px 0 2px 12px;
    padding-left: 9px;
    border-left: 1px solid var(--color-text-tertiary);
  }
  .unfiled-label {
    margin-top: 20px;
  }

  .unfiled-group,
  .folder-row.drag-over,
  .unfiled-group.drag-over {
    border-radius: var(--radius-sm);
  }

  .folder-row.drag-over,
  .unfiled-group.drag-over {
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
    background: var(--color-selection-bg);
  }

  /* Nothing unread: stays listed (so the sidebar doesn't reshuffle) but
     recedes, letting feeds with something to read stand out. */
  .row.quiet:not(.selected) .title {
    color: var(--color-text-secondary);
  }

  .caught-up {
    margin: var(--space-4) var(--space-3);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }
</style>
