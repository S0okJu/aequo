<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';
  import FeedIcon from '../FeedIcon.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import {
    createFeedFolder,
    deleteFeed,
    deleteFeedFolder,
    refreshFeed,
    renameFeed,
    renameFeedFolder,
    setFeedRefreshInterval,
    updateFeedOrganization,
  } from '$lib/api';
  import { feedDisplayName, formatDateTime } from '$lib/format';
  import { t } from '$lib/i18n/index.svelte';
  import type { FeedFolder, FeedSummary } from '$lib/types';

  interface Props {
    feeds: FeedSummary[];
    folders: FeedFolder[];
    /** The feed whose details are open; bindable so the page can preselect one. */
    selectedId?: number | null;
    onChanged: () => void | Promise<void>;
    onAddFeed: () => void;
  }

  let { feeds, folders, selectedId = $bindable(null), onChanged, onAddFeed }: Props = $props();

  const INTERVALS = [300, 900, 1800, 3600, 21600, 43200, 86400];

  let query = $state('');
  let searchInput = $state<HTMLInputElement | undefined>(undefined);
  const selected = $derived(feeds.find((feed) => feed.id === selectedId) ?? null);

  function intervalLabel(seconds: number): string {
    if (seconds >= 86400) return t('settings.everyDay');
    if (seconds % 3600 === 0) return t('settings.everyHours', { count: seconds / 3600 });
    return t('settings.everyMinutes', { count: Math.round(seconds / 60) });
  }

  function hasError(feed: FeedSummary): boolean {
    return feed.lastError !== null || feed.consecutiveFailures > 0;
  }

  const normalizedQuery = $derived(query.trim().toLowerCase());
  function matches(feed: FeedSummary): boolean {
    if (!normalizedQuery) return true;
    return [feedDisplayName(feed), feed.title ?? '', feed.url].some((text) =>
      text.toLowerCase().includes(normalizedQuery),
    );
  }
  const matching = $derived(feeds.filter(matches));
  const erroring = $derived(matching.filter(hasError));
  const byFolder = $derived.by(() => {
    const groups: Record<number, FeedSummary[]> = {};
    for (const feed of matching) {
      if (feed.folderId !== null) (groups[feed.folderId] ??= []).push(feed);
    }
    return groups;
  });
  const unfiled = $derived(matching.filter((feed) => feed.folderId === null));

  // A feed hidden by the search can't stay "selected" behind the scenes.
  $effect(() => {
    if (selected && !matches(selected)) selectedId = null;
  });

  function toggleSelect(feedId: number) {
    selectedId = selectedId === feedId ? null : feedId;
  }

  function closeDetail() {
    const closedId = selectedId;
    selectedId = null;
    queueMicrotask(() =>
      document.querySelector<HTMLElement>(`[data-feed-row="${closedId}"]`)?.focus(),
    );
  }

  // --- Feed detail -----------------------------------------------------

  let nameDraft = $state('');
  let saveStatus = $state<{ text: string; error: boolean } | null>(null);
  let refreshing = new SvelteSet<number>();
  let confirmingUnsubscribe = $state(false);
  let removing = $state(false);
  let copied = $state(false);

  // Reset per-feed editing state when another feed is selected.
  $effect(() => {
    void selectedId;
    nameDraft = selected?.customTitle ?? '';
    saveStatus = null;
    confirmingUnsubscribe = false;
    copied = false;
  });

  const nameDirty = $derived(
    selected !== null && nameDraft.trim() !== (selected.customTitle ?? ''),
  );

  async function save(action: () => Promise<unknown>) {
    saveStatus = null;
    try {
      await action();
      await onChanged();
      saveStatus = { text: t('settings.saved'), error: false };
    } catch (error) {
      saveStatus = { text: t('settings.saveFailed', { error: String(error) }), error: true };
    }
  }

  async function refreshSelected(feed: FeedSummary) {
    refreshing.add(feed.id);
    saveStatus = null;
    try {
      await refreshFeed(feed.id);
      await onChanged();
    } catch (error) {
      saveStatus = { text: String(error), error: true };
    } finally {
      refreshing.delete(feed.id);
    }
  }

  async function unsubscribe(feed: FeedSummary) {
    removing = true;
    try {
      await deleteFeed(feed.id);
      selectedId = null;
      await onChanged();
    } catch (error) {
      saveStatus = { text: String(error), error: true };
    } finally {
      removing = false;
    }
  }

  async function copyUrl(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      copied = true;
    } catch {
      copied = false;
    }
  }

  // --- Folders -----------------------------------------------------------

  let creatingFolder = $state(false);
  let newFolderName = $state('');
  let renamingFolderId = $state<number | null>(null);
  let renameValue = $state('');
  let confirmingDeleteFolderId = $state<number | null>(null);
  let folderBusy = $state(false);
  let folderError = $state<{ folderId: number | 'new'; text: string } | null>(null);

  function folderNameError(name: string, excludeId?: number): string | null {
    if (!name) return t('subs.folderEmptyName');
    const normalized = name.toLowerCase();
    return folders.some((f) => f.id !== excludeId && f.name.trim().toLowerCase() === normalized)
      ? t('subs.folderExists', { name })
      : null;
  }

  async function runFolderAction(target: number | 'new', action: () => Promise<unknown>) {
    folderBusy = true;
    folderError = null;
    try {
      await action();
      await onChanged();
      return true;
    } catch (error) {
      const raw = String(error);
      folderError = {
        folderId: target,
        text: raw.includes('UNIQUE constraint') ? t('subs.folderError') : raw,
      };
      return false;
    } finally {
      folderBusy = false;
    }
  }

  async function submitNewFolder() {
    const name = newFolderName.trim();
    const problem = folderNameError(name);
    if (problem) {
      folderError = { folderId: 'new', text: problem };
      return;
    }
    if (await runFolderAction('new', () => createFeedFolder(name))) {
      creatingFolder = false;
      newFolderName = '';
    }
  }

  async function submitRename(folder: FeedFolder) {
    const name = renameValue.trim();
    if (name === folder.name) {
      renamingFolderId = null;
      return;
    }
    const problem = folderNameError(name, folder.id);
    if (problem) {
      folderError = { folderId: folder.id, text: problem };
      return;
    }
    if (await runFolderAction(folder.id, () => renameFeedFolder(folder.id, name))) {
      renamingFolderId = null;
    }
  }

  async function confirmDeleteFolder(folder: FeedFolder) {
    if (await runFolderAction(folder.id, () => deleteFeedFolder(folder.id))) {
      confirmingDeleteFolderId = null;
    }
  }

  function autofocus(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function handleKeydown(event: KeyboardEvent) {
    // Escape closes the innermost thing first: an inline editor, then the
    // detail panel; only then does the page take it (to leave settings).
    if (event.key !== 'Escape' || event.defaultPrevented) return;
    if (creatingFolder || renamingFolderId !== null || confirmingDeleteFolderId !== null) {
      event.preventDefault();
      creatingFolder = false;
      renamingFolderId = null;
      confirmingDeleteFolderId = null;
      folderError = null;
    } else if (selectedId !== null) {
      event.preventDefault();
      closeDetail();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#snippet feedRow(feed: FeedSummary)}
  <button
    type="button"
    class="feed-row"
    class:selected={selectedId === feed.id}
    aria-current={selectedId === feed.id ? 'true' : undefined}
    data-feed-row={feed.id}
    title={feed.url}
    onclick={() => toggleSelect(feed.id)}
  >
    <FeedIcon {feed} />
    <span class="feed-name">{feedDisplayName(feed)}</span>
    {#if hasError(feed)}
      <span class="feed-error" role="img" aria-label={t('subs.needsAttention')}
        ><Icon name="error" size={12} /></span
      >
    {/if}
    <span class="feed-interval">{intervalLabel(feed.intervalSecs)}</span>
  </button>
{/snippet}

<div class="subscriptions" class:with-detail={selected !== null}>
  <div class="list-pane">
    <div class="toolbar">
      <div class="search">
        <Icon name="search" size={14} />
        <input
          bind:this={searchInput}
          bind:value={query}
          type="search"
          aria-label={t('subs.search')}
          placeholder={t('subs.searchPlaceholder')}
        />
        {#if query}
          <button
            type="button"
            class="icon-button small"
            aria-label={t('subs.clearSearch')}
            title={t('subs.clearSearch')}
            onclick={() => {
              query = '';
              searchInput?.focus();
            }}><Icon name="close" size={12} /></button
          >
        {/if}
      </div>
      <span class="count" role="status">
        {normalizedQuery
          ? t('subs.results', { count: matching.length })
          : t('subs.count', { count: feeds.length })}
      </span>
      <button type="button" class="primary" onclick={onAddFeed}>
        <Icon name="plus" size={14} />{t('addFeed.title')}
      </button>
    </div>

    <div class="list">
      {#if feeds.length === 0}
        <div class="empty-placeholder">
          <span class="empty-placeholder-icon"><Icon name="empty" size={20} /></span>
          <h3>{t('subs.emptyTitle')}</h3>
          <p>{t('subs.emptyBody')}</p>
        </div>
      {:else if matching.length === 0}
        <p class="no-results">{t('subs.noResults', { query: query.trim() })}</p>
      {/if}

      {#if erroring.length > 0}
        <section class="group attention" aria-label={t('subs.needsAttention')}>
          <h3 class="group-title">{t('subs.needsAttention')}</h3>
          {#each erroring as feed (feed.id)}{@render feedRow(feed)}{/each}
        </section>
      {/if}

      {#each folders as folder (folder.id)}
        {@const folderFeeds = byFolder[folder.id] ?? []}
        {#if !normalizedQuery || folderFeeds.length > 0}
          <section class="group" aria-label={folder.name}>
            {#if renamingFolderId === folder.id}
              <form
                class="inline-form"
                onsubmit={(event) => {
                  event.preventDefault();
                  submitRename(folder);
                }}
              >
                <input
                  type="text"
                  bind:value={renameValue}
                  aria-label={t('subs.folderName')}
                  use:autofocus
                  disabled={folderBusy}
                />
                <button type="submit" class="primary small" disabled={folderBusy}
                  >{folderBusy ? t('subs.saving') : t('subs.save')}</button
                >
                <button type="button" class="small" onclick={() => (renamingFolderId = null)}
                  >{t('common.cancel')}</button
                >
              </form>
            {:else}
              <div class="group-header">
                <h3 class="group-title">{folder.name}</h3>
                <div class="group-actions">
                  <button
                    type="button"
                    class="icon-button small"
                    aria-label={t('subs.renameFolder', { name: folder.name })}
                    title={t('subs.rename')}
                    onclick={() => {
                      renamingFolderId = folder.id;
                      renameValue = folder.name;
                      folderError = null;
                    }}><Icon name="edit" size={13} /></button
                  >
                  <button
                    type="button"
                    class="icon-button small"
                    aria-label={t('subs.deleteFolder', { name: folder.name })}
                    title={t('subs.deleteFolder', { name: folder.name })}
                    onclick={() => {
                      confirmingDeleteFolderId = folder.id;
                      folderError = null;
                    }}><Icon name="close" size={13} /></button
                  >
                </div>
              </div>
            {/if}
            {#if confirmingDeleteFolderId === folder.id}
              {@const count = feeds.filter((feed) => feed.folderId === folder.id).length}
              <div class="confirm-row">
                <span>{t('subs.deleteFolderConfirm', { name: folder.name, count })}</span>
                <div class="confirm-actions">
                  <button type="button" onclick={() => (confirmingDeleteFolderId = null)}
                    >{t('common.cancel')}</button
                  >
                  <button
                    type="button"
                    class="confirm-remove"
                    disabled={folderBusy}
                    onclick={() => confirmDeleteFolder(folder)}
                    >{folderBusy ? t('subs.deleting') : t('common.delete')}</button
                  >
                </div>
              </div>
            {/if}
            {#if folderError?.folderId === folder.id}
              <p class="field-error" role="alert">{folderError.text}</p>
            {/if}
            {#each folderFeeds as feed (feed.id)}{@render feedRow(feed)}{/each}
            {#if folderFeeds.length === 0}
              <p class="group-empty">{t('subs.emptyFolder')}</p>
            {/if}
          </section>
        {/if}
      {/each}

      {#if unfiled.length > 0}
        <section class="group" aria-label={t('subs.unfiled')}>
          {#if folders.length > 0}<h3 class="group-title">{t('subs.unfiled')}</h3>{/if}
          {#each unfiled as feed (feed.id)}{@render feedRow(feed)}{/each}
        </section>
      {/if}

      {#if !normalizedQuery}
        {#if creatingFolder}
          <form
            class="inline-form new-folder-form"
            onsubmit={(event) => {
              event.preventDefault();
              submitNewFolder();
            }}
          >
            <input
              type="text"
              bind:value={newFolderName}
              aria-label={t('subs.folderName')}
              placeholder={t('subs.folderName')}
              use:autofocus
              disabled={folderBusy}
            />
            <button type="submit" class="primary small" disabled={folderBusy}
              >{folderBusy ? t('subs.creating') : t('subs.create')}</button
            >
            <button
              type="button"
              class="small"
              onclick={() => {
                creatingFolder = false;
                folderError = null;
              }}>{t('common.cancel')}</button
            >
          </form>
          {#if folderError?.folderId === 'new'}
            <p class="field-error" role="alert">{folderError.text}</p>
          {/if}
        {:else}
          <button
            type="button"
            class="new-folder"
            onclick={() => {
              creatingFolder = true;
              newFolderName = '';
              folderError = null;
            }}><Icon name="plus" size={13} />{t('subs.newFolder')}</button
          >
        {/if}
      {/if}
    </div>
  </div>

  {#if selected}
    {@const feed = selected}
    <aside class="detail" aria-labelledby="feed-detail-title">
      <header class="detail-header">
        <FeedIcon {feed} size={20} />
        <h2 id="feed-detail-title">{feedDisplayName(feed)}</h2>
        <button
          type="button"
          class="icon-button"
          aria-label={t('subs.closeDetail')}
          title={t('subs.closeDetail')}
          onclick={closeDetail}><Icon name="close" size={15} /></button
        >
      </header>

      <div class="detail-body">
        <form
          class="field"
          onsubmit={(event) => {
            event.preventDefault();
            save(() => renameFeed(feed.id, nameDraft.trim() || null));
          }}
        >
          <label for="feed-display-name">{t('subs.displayName')}</label>
          <div class="field-row">
            <input
              id="feed-display-name"
              type="text"
              bind:value={nameDraft}
              placeholder={feed.title ?? feed.url}
            />
            {#if nameDirty}
              <button type="submit" class="primary small">{t('subs.save')}</button>
            {/if}
          </div>
          <p class="field-hint">{t('subs.displayNameHint')}</p>
        </form>

        <div class="field">
          <span class="field-label">{t('subs.url')}</span>
          <div class="field-row">
            <code class="url">{feed.url}</code>
            <button
              type="button"
              class="icon-button small"
              aria-label={copied ? t('subs.copied') : t('subs.copyUrl')}
              title={copied ? t('subs.copied') : t('subs.copyUrl')}
              onclick={() => copyUrl(feed.url)}
              ><Icon name={copied ? 'check' : 'copy'} size={13} /></button
            >
          </div>
        </div>

        <div class="field">
          <label for="feed-folder">{t('subs.folder')}</label>
          <select
            id="feed-folder"
            value={feed.folderId ?? ''}
            onchange={(event) => {
              const value = event.currentTarget.value;
              save(() =>
                updateFeedOrganization(
                  feed.id,
                  value === '' ? null : Number(value),
                  feed.hiddenFromAll,
                ),
              );
            }}
          >
            <option value="">{t('subs.unfiled')}</option>
            {#each folders as folder (folder.id)}
              <option value={folder.id}>{folder.name}</option>
            {/each}
          </select>
        </div>

        <div class="field">
          <label for="feed-interval">{t('subs.interval')}</label>
          <select
            id="feed-interval"
            value={feed.intervalSecs}
            onchange={(event) => {
              const seconds = Number(event.currentTarget.value);
              save(() => setFeedRefreshInterval(feed.id, seconds));
            }}
          >
            {#each INTERVALS as seconds (seconds)}
              <option value={seconds}>{intervalLabel(seconds)}</option>
            {/each}
          </select>
        </div>

        <label class="switch-field">
          <span>
            <span class="field-label">{t('subs.showInAll')}</span>
            <span class="field-hint">{t('subs.showInAllHint')}</span>
          </span>
          <input
            type="checkbox"
            role="switch"
            checked={!feed.hiddenFromAll}
            onchange={(event) => {
              const shown = event.currentTarget.checked;
              save(() => updateFeedOrganization(feed.id, feed.folderId, !shown));
            }}
          />
        </label>

        {#if saveStatus}
          <p class="save-status" class:error={saveStatus.error} role="status">{saveStatus.text}</p>
        {/if}

        <div class="field">
          <span class="field-label">{t('subs.status')}</span>
          <p class="status-line">
            {t('subs.unread', { count: feed.unreadCount })} ·
            {t('subs.nextCheck', {
              time: feed.nextFetchAt > 0 ? formatDateTime(feed.nextFetchAt) : t('subs.soon'),
            })}
          </p>
          {#if feed.lastError}
            <p class="status-error">{t('subs.lastError', { error: feed.lastError })}</p>
          {/if}
        </div>

        <div class="detail-actions">
          <button
            type="button"
            class="small"
            disabled={refreshing.has(feed.id)}
            onclick={() => refreshSelected(feed)}
            >{refreshing.has(feed.id) ? t('subs.refreshing') : t('subs.refreshNow')}</button
          >
          {#if !confirmingUnsubscribe}
            <button
              type="button"
              class="small danger"
              onclick={() => (confirmingUnsubscribe = true)}>{t('subs.unsubscribe')}</button
            >
          {/if}
        </div>
        {#if confirmingUnsubscribe}
          <div class="confirm-row">
            <span>{t('subs.unsubscribeConfirm', { name: feedDisplayName(feed) })}</span>
            <div class="confirm-actions">
              <button type="button" onclick={() => (confirmingUnsubscribe = false)}
                >{t('common.cancel')}</button
              >
              <button
                type="button"
                class="confirm-remove"
                disabled={removing}
                onclick={() => unsubscribe(feed)}
                >{removing ? t('subs.removing') : t('subs.unsubscribe')}</button
              >
            </div>
          </div>
        {/if}
      </div>
    </aside>
  {/if}
</div>

<style>
  .subscriptions {
    position: relative;
    display: grid;
    min-height: 0;
    flex: 1;
    grid-template-columns: minmax(0, 1fr);
    container-type: inline-size;
  }

  .subscriptions.with-detail {
    grid-template-columns: minmax(280px, 1fr) minmax(320px, 360px);
  }

  .list-pane {
    display: flex;
    min-height: 0;
    flex-direction: column;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding-bottom: var(--space-3);
  }

  .search {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-1) 0 var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text-tertiary);
  }

  .search:focus-within {
    border-color: var(--color-accent);
  }

  .search input {
    min-width: 0;
    flex: 1;
    height: 32px;
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

  .count {
    flex-shrink: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .list {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: var(--space-4);
    overflow-y: auto;
    padding-bottom: var(--space-5);
  }

  .group {
    display: flex;
    flex-direction: column;
  }

  .group-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .group-title {
    margin: 0 0 var(--space-1);
    padding: 0 var(--space-2);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    font-weight: var(--weight-medium);
  }

  .attention .group-title {
    color: var(--color-error);
  }

  .group-actions {
    display: flex;
    gap: 2px;
    opacity: 0;
  }

  .group-header:hover .group-actions,
  .group-header:focus-within .group-actions {
    opacity: 1;
  }

  @media (hover: none) {
    .group-actions {
      opacity: 1;
    }
  }

  .group-empty,
  .no-results {
    margin: var(--space-1) var(--space-2);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .feed-row {
    display: flex;
    min-height: 34px;
    align-items: center;
    gap: var(--space-2);
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

  .feed-row:hover {
    background: var(--color-hover-bg);
  }

  .feed-row.selected {
    background: var(--color-selection-bg);
  }

  .feed-name {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .feed-error {
    display: inline-flex;
    color: var(--color-error);
  }

  .feed-interval {
    flex-shrink: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .new-folder {
    display: inline-flex;
    align-self: flex-start;
    align-items: center;
    gap: var(--space-1);
    padding: 4px var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  .new-folder:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  .inline-form {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-2) var(--space-1);
  }

  .inline-form input,
  .field input[type='text'],
  .field select {
    min-width: 0;
    flex: 1;
    padding: 6px 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
  }

  .field-error {
    margin: 0 var(--space-2) var(--space-1);
    color: var(--color-error);
    font-size: var(--text-small);
  }

  .detail {
    display: flex;
    min-height: 0;
    flex-direction: column;
    margin-left: var(--space-5);
    border-left: 1px solid var(--color-border);
    background: var(--color-bg);
  }

  /* Too narrow for list + detail side by side: the detail floats over the list. */
  @container (max-width: 700px) {
    .subscriptions.with-detail {
      grid-template-columns: minmax(0, 1fr);
    }

    .detail {
      position: absolute;
      top: 0;
      right: 0;
      bottom: 0;
      width: min(360px, 100%);
      margin-left: 0;
      box-shadow: var(--shadow-popover);
    }
  }

  .detail-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-2) var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
  }

  .detail-header h2 {
    min-width: 0;
    flex: 1;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    overflow-y: auto;
    padding: var(--space-4);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .field > label,
  .field-label {
    color: var(--color-text);
    font-size: var(--text-small);
    font-weight: var(--weight-medium);
  }

  .field-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .field-hint {
    margin: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .url {
    min-width: 0;
    flex: 1;
    overflow-wrap: anywhere;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .switch-field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    cursor: pointer;
  }

  .switch-field > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .switch-field input {
    width: 18px;
    height: 18px;
    accent-color: var(--color-accent-solid);
  }

  .save-status {
    margin: 0;
    color: var(--color-success);
    font-size: var(--text-small);
  }

  .save-status.error,
  .status-error {
    color: var(--color-error);
  }

  .status-line,
  .status-error {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.45;
    overflow-wrap: anywhere;
  }

  .status-error {
    color: var(--color-error);
  }

  .detail-actions {
    display: flex;
    gap: var(--space-2);
  }

  .icon-button {
    display: grid;
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
  }

  .icon-button.small {
    width: 24px;
    height: 24px;
  }

  .icon-button:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  button.primary,
  button.small:not(.icon-button) {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    gap: var(--space-1);
    padding: 6px var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    cursor: pointer;
  }

  button.small:not(.icon-button) {
    padding: 4px var(--space-2);
    font-size: var(--text-small);
  }

  button.primary {
    border-color: var(--color-accent-solid);
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
  }

  button.small:not(.icon-button):hover:not(:disabled) {
    background: var(--color-hover-bg);
  }

  button.primary:hover:not(:disabled) {
    background: var(--color-accent-solid);
    filter: brightness(0.95);
  }

  button.danger {
    color: var(--color-error);
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
