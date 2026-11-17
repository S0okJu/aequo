<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';
  import AddFeedForm from './AddFeedForm.svelte';
  import FolderSettings from './FolderSettings.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import { applyThemePreference, getThemePreference, type ThemePreference } from '$lib/theme';
  import { showToast } from '$lib/toast.svelte.ts';
  import {
    deleteFeed,
    refreshFeed,
    getRetentionSettings,
    setRetentionSettings,
    previewRetentionCleanup,
    runRetentionCleanup,
    updateFeedOrganization,
    setFeedRefreshInterval,
  } from '$lib/api';
  import type { FeedFolder, FeedSummary } from '$lib/types';

  type SettingsTab = 'feeds' | 'folders' | 'storage' | 'appearance';

  interface Props {
    feeds: FeedSummary[];
    folders: FeedFolder[];
    onAdded: () => void;
    onChanged: () => void | Promise<void>;
    onBack: () => void;
    initialTab?: SettingsTab;
  }

  let { feeds, folders, onAdded, onChanged, onBack, initialTab = 'feeds' }: Props = $props();
  // Settings is remounted fresh each time it's opened (+page.svelte's
  // {#if settingsOpen} block), so reading `initialTab` only at mount time
  // to seed the active tab is safe - there's no later value to miss.
  // svelte-ignore state_referenced_locally
  let activeTab = $state<SettingsTab>(initialTab);
  let refreshing = new SvelteSet<number>();
  let deleting = new SvelteSet<number>();
  let savingSettings = new SvelteSet<number>();
  let confirmingDeleteId = $state<number | null>(null);
  let errors = $state<Record<number, string>>({});
  let retention = $state({ readRetentionDays: 365, fullTextRetentionDays: 180 });
  let cleanupPreview = $state({
    articlesToRemove: 0,
    fullTextsToClear: 0,
    estimatedBytesToFree: 0,
  });
  let storageBusy = $state(false);
  let storageError = $state('');
  let themePreference = $derived<ThemePreference>(getThemePreference());

  function updateThemePreference(preference: ThemePreference) {
    themePreference = preference;
    applyThemePreference(preference);
  }

  $effect(() => {
    getRetentionSettings()
      .then((value) => (retention = value))
      .catch((error) => (storageError = String(error)));
    previewRetentionCleanup()
      .then((value) => (cleanupPreview = value))
      .catch((error) => (storageError = String(error)));
  });

  async function updateRetention() {
    storageBusy = true;
    storageError = '';
    try {
      await setRetentionSettings(retention);
      cleanupPreview = await previewRetentionCleanup();
      showToast('Retention policy saved.', { type: 'success' });
    } catch (error) {
      storageError = String(error);
    } finally {
      storageBusy = false;
    }
  }

  async function cleanNow() {
    storageBusy = true;
    storageError = '';
    try {
      const result = await runRetentionCleanup();
      cleanupPreview = await previewRetentionCleanup();
      showToast(
        `Removed ${result.articlesRemoved} articles and cleared ${result.fullTextsCleared} full texts (${formatBytes(result.bytesFreed)}).`,
        { type: 'success' },
      );
      onChanged();
    } catch (error) {
      storageError = String(error);
    } finally {
      storageBusy = false;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function daysLabel(days: number): string {
    return days === 0 ? 'Never' : `${days} days`;
  }

  function intervalLabel(seconds: number): string {
    if (seconds % 3600 === 0) return `${seconds / 3600} hour${seconds === 3600 ? '' : 's'}`;
    return `${Math.round(seconds / 60)} minutes`;
  }

  function nextCheckLabel(timestamp: number): string {
    if (timestamp <= 0) return 'Soon';
    return new Date(timestamp * 1000).toLocaleString(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short',
    });
  }

  async function refresh(id: number) {
    refreshing.add(id);
    errors = { ...errors, [id]: '' };
    try {
      await refreshFeed(id);
      onChanged();
    } catch (error) {
      errors = { ...errors, [id]: String(error) };
    } finally {
      refreshing.delete(id);
    }
  }

  async function remove(feed: FeedSummary) {
    errors = { ...errors, [feed.id]: '' };
    deleting.add(feed.id);
    try {
      await deleteFeed(feed.id);
      confirmingDeleteId = null;
      await onChanged();
    } catch (error) {
      errors = { ...errors, [feed.id]: String(error) };
    } finally {
      deleting.delete(feed.id);
    }
  }

  async function updateOrganization(
    feed: FeedSummary,
    folderId: number | null,
    hiddenFromAll: boolean,
  ) {
    errors = { ...errors, [feed.id]: '' };
    savingSettings.add(feed.id);
    try {
      await updateFeedOrganization(feed.id, folderId, hiddenFromAll);
      await onChanged();
    } catch (error) {
      errors = { ...errors, [feed.id]: String(error) };
    } finally {
      savingSettings.delete(feed.id);
    }
  }

  async function updateInterval(feed: FeedSummary, intervalSecs: number) {
    errors = { ...errors, [feed.id]: '' };
    savingSettings.add(feed.id);
    try {
      await setFeedRefreshInterval(feed.id, intervalSecs);
      await onChanged();
    } catch (error) {
      errors = { ...errors, [feed.id]: String(error) };
    } finally {
      savingSettings.delete(feed.id);
    }
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key !== 'Escape' || event.defaultPrevented) return;
    const active = document.activeElement;
    if (active && ['INPUT', 'TEXTAREA', 'SELECT'].includes(active.tagName)) return;
    onBack();
  }}
/>

<section class="settings" aria-labelledby="settings-title">
  <header class="header">
    <button class="back" type="button" onclick={onBack} aria-label="Back to reading">
      <Icon name="chevron-left" size={14} />
      <span>Back to reading</span>
    </button>
    <div>
      <p class="eyebrow">PREFERENCES</p>
      <h1 id="settings-title">Feed settings</h1>
      <p class="intro">Add subscriptions, organize folders, and manage storage.</p>
    </div>
  </header>

  <div class="tabs" role="tablist" aria-label="Settings sections">
    <button
      type="button"
      role="tab"
      aria-selected={activeTab === 'feeds'}
      class:active={activeTab === 'feeds'}
      onclick={() => (activeTab = 'feeds')}>Feeds</button
    >
    <button
      type="button"
      role="tab"
      aria-selected={activeTab === 'folders'}
      class:active={activeTab === 'folders'}
      onclick={() => (activeTab = 'folders')}>Folders</button
    >
    <button
      type="button"
      role="tab"
      aria-selected={activeTab === 'storage'}
      class:active={activeTab === 'storage'}
      onclick={() => (activeTab = 'storage')}>Storage</button
    >
    <button
      type="button"
      role="tab"
      aria-selected={activeTab === 'appearance'}
      class:active={activeTab === 'appearance'}
      onclick={() => (activeTab = 'appearance')}>Appearance</button
    >
  </div>

  {#if activeTab === 'feeds'}
    <div class="tab-panel" role="tabpanel" aria-label="Feeds">
      <section class="add-section" aria-labelledby="add-title">
        <h2 id="add-title">Add a feed</h2>
        <p>
          Paste an RSS, Atom, JSON Feed, or public article-list page URL. Pages without a feed are
          checked daily by default.
        </p>
        <AddFeedForm {onAdded} />
      </section>

      <section class="subscriptions" aria-labelledby="subscriptions-title">
        <div class="section-heading">
          <div>
            <h2 id="subscriptions-title">Subscriptions</h2>
            <p>{feeds.length} {feeds.length === 1 ? 'feed' : 'feeds'}</p>
          </div>
        </div>

        {#if feeds.length === 0}
          <div class="empty-placeholder">
            <span class="empty-placeholder-icon"><Icon name="empty" size={20} /></span>
            <h3>No feeds yet</h3>
            <p>Add one above to get started.</p>
          </div>
        {:else}
          <ul class="feed-list">
            {#each feeds as feed (feed.id)}
              <li class="feed-card">
                <div class="feed-info">
                  <div class="identity">
                    <h3>{feed.title ?? feed.url}</h3>
                    <p class="url">{feed.url}</p>
                  </div>
                  <div class="meta-pills">
                    <span class="meta-pill">{feed.format?.toUpperCase() ?? 'FEED'}</span>
                    <span class="meta-pill">Every {intervalLabel(feed.intervalSecs)}</span>
                    <span class="meta-pill" class:meta-pill-accent={feed.unreadCount > 0}
                      >{feed.unreadCount} unread</span
                    >
                    <span class="meta-pill">Next check {nextCheckLabel(feed.nextFetchAt)}</span>
                  </div>
                  <div class="organization-controls">
                    <label class="setting-control"
                      ><span>Folder</span>
                      <select
                        value={feed.folderId ?? ''}
                        disabled={deleting.has(feed.id) || savingSettings.has(feed.id)}
                        onchange={(event) =>
                          updateOrganization(
                            feed,
                            event.currentTarget.value === ''
                              ? null
                              : Number(event.currentTarget.value),
                            feed.hiddenFromAll,
                          )}
                      >
                        <option value="">Unfiled</option>
                        {#each folders as folder (folder.id)}<option value={folder.id}
                            >{folder.name}</option
                          >{/each}
                      </select>
                    </label>
                    <label class="setting-control"
                      ><span>Refresh every</span>
                      <select
                        value={feed.intervalSecs}
                        aria-label={`Refresh interval for ${feed.title ?? feed.url}`}
                        disabled={deleting.has(feed.id) || savingSettings.has(feed.id)}
                        onchange={(event) =>
                          updateInterval(feed, Number(event.currentTarget.value))}
                      >
                        {#each [[300, '5 minutes'], [900, '15 minutes'], [1800, '30 minutes'], [3600, '1 hour'], [21600, '6 hours'], [43200, '12 hours'], [86400, '1 day']] as [seconds, label] (seconds)}
                          <option value={seconds}>{label}</option>
                        {/each}
                      </select>
                    </label>
                    <label class="visibility-control" title="Include this feed in the All view">
                      <input
                        type="checkbox"
                        role="switch"
                        checked={!feed.hiddenFromAll}
                        aria-label={`Show ${feed.title ?? feed.url} in All`}
                        disabled={deleting.has(feed.id) || savingSettings.has(feed.id)}
                        onchange={(event) =>
                          updateOrganization(feed, feed.folderId, !event.currentTarget.checked)}
                      />
                      <span class="switch-track" aria-hidden="true"><span></span></span>
                      <span class="visibility-label">Show in All</span>
                    </label>
                  </div>
                  {#if feed.lastError}
                    <div class="error-card feed-error">
                      <span class="error-mark"><Icon name="error" size={12} /></span>
                      <div class="error-copy"><p>Last refresh failed: {feed.lastError}</p></div>
                    </div>
                  {/if}
                  {#if errors[feed.id]}
                    <div class="error-card feed-error">
                      <span class="error-mark"><Icon name="error" size={12} /></span>
                      <div class="error-copy"><p>{errors[feed.id]}</p></div>
                    </div>
                  {/if}
                  {#if confirmingDeleteId === feed.id}
                    <div class="confirm-row">
                      <span
                        >Unsubscribe from "{feed.title ?? feed.url}"? This removes it and its stored
                        articles.</span
                      >
                      <div class="confirm-actions">
                        <button type="button" onclick={() => (confirmingDeleteId = null)}
                          >Cancel</button
                        >
                        <button
                          type="button"
                          class="confirm-remove"
                          disabled={deleting.has(feed.id)}
                          onclick={() => remove(feed)}
                        >
                          {deleting.has(feed.id) ? 'Removing…' : 'Unsubscribe'}
                        </button>
                      </div>
                    </div>
                  {/if}
                </div>
                <div class="actions">
                  <button
                    type="button"
                    disabled={refreshing.has(feed.id)}
                    onclick={() => refresh(feed.id)}
                  >
                    {refreshing.has(feed.id) ? 'Refreshing…' : 'Refresh'}
                  </button>
                  {#if confirmingDeleteId !== feed.id}
                    <button
                      class="unsubscribe"
                      type="button"
                      onclick={() => (confirmingDeleteId = feed.id)}>Unsubscribe</button
                    >
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    </div>
  {:else if activeTab === 'folders'}
    <div class="tab-panel" role="tabpanel" aria-label="Folders">
      <FolderSettings {feeds} {folders} onFoldersChanged={onChanged} />
    </div>
  {:else if activeTab === 'storage'}
    <div class="tab-panel" role="tabpanel" aria-label="Storage">
      <section class="storage" aria-labelledby="storage-title">
        <div class="section-heading">
          <div>
            <h2 id="storage-title">Storage &amp; retention</h2>
            <p>Keep old posts manageable while protecting unread and saved articles.</p>
          </div>
        </div>
        <div class="retention-fields">
          <label
            >Read article metadata
            <select bind:value={retention.readRetentionDays}>
              {#each [0, 90, 180, 365] as days (days)}<option value={days}>{daysLabel(days)}</option
                >{/each}
            </select>
            <small>After this period, read posts and their Aequo session links are removed.</small>
          </label>
          <label
            >Full text and extracted cache
            <select bind:value={retention.fullTextRetentionDays}>
              {#each [0, 30, 90, 180, 365] as days (days)}<option value={days}
                  >{daysLabel(days)}</option
                >{/each}
            </select>
            <small>Clearing full text keeps the post title, summary, and original link.</small>
          </label>
        </div>
        <p class="retention-note">
          Unread and saved posts are always kept. Removed session links do not delete Codex or
          Claude transcripts.
        </p>
        <p class="cleanup-preview">
          Current cleanup: {cleanupPreview.articlesToRemove} posts to remove · {cleanupPreview.fullTextsToClear}
          full texts to clear · about {formatBytes(cleanupPreview.estimatedBytesToFree)} to free
        </p>
        {#if storageError}
          <div class="error-card">
            <span class="error-mark"><Icon name="error" size={12} /></span>
            <div class="error-copy"><p>{storageError}</p></div>
          </div>
        {/if}
        <div class="storage-actions">
          <button type="button" disabled={storageBusy} onclick={updateRetention}>Save policy</button
          ><button type="button" disabled={storageBusy} onclick={cleanNow}>Clean up now</button>
        </div>
      </section>
    </div>
  {:else}
    <div class="tab-panel" role="tabpanel" aria-label="Appearance">
      <section class="appearance" aria-labelledby="appearance-title">
        <div class="section-heading">
          <div>
            <h2 id="appearance-title">Appearance</h2>
            <p>Choose how Aequo looks on this device.</p>
          </div>
        </div>
        <label class="theme-control">
          <span>Theme</span>
          <select
            value={themePreference}
            aria-label="Theme"
            onchange={(event) =>
              updateThemePreference(event.currentTarget.value as ThemePreference)}
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
          <small>System follows your operating system's appearance setting.</small>
        </label>
      </section>
    </div>
  {/if}
</section>

<style>
  .settings {
    width: min(920px, 100%);
    min-height: 100%;
    margin-inline: auto;
    padding: clamp(24px, 5vw, 48px);
    color: var(--color-text);
  }

  .header,
  .section-heading,
  .feed-card {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 20px;
  }

  .header {
    position: sticky;
    top: 0;
    z-index: 5;
    flex-direction: column;
    align-items: flex-start;
    gap: 20px;
    padding-block: 10px;
    margin-bottom: 24px;
    background: var(--color-bg-secondary);
  }

  .header > div {
    width: 100%;
  }

  .eyebrow {
    margin: 0 0 8px;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    font-weight: var(--weight-bold);
    letter-spacing: 0.08em;
  }

  h1,
  h2,
  h3,
  p {
    margin-top: 0;
  }

  h1 {
    margin-bottom: 6px;
    font-size: var(--text-display);
    letter-spacing: -0.025em;
  }

  h2 {
    margin-bottom: 5px;
    font-size: var(--text-heading);
  }

  .intro,
  .add-section > p,
  .section-heading p {
    margin-bottom: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-body);
  }

  button {
    min-height: 34px;
    padding: 6px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: var(--color-hover-bg);
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .back {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--color-text-secondary);
    font-weight: var(--weight-medium);
  }

  .back:hover:not(:disabled) {
    border-color: var(--color-border);
    background: var(--color-bg);
    color: var(--color-text);
  }

  .tabs {
    display: flex;
    gap: 18px;
    margin-bottom: 24px;
    padding-bottom: 0;
    border-bottom: 1px solid var(--color-border);
  }

  .tabs button {
    min-height: auto;
    padding: 0 1px 11px;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: transparent;
    color: var(--color-text-tertiary);
    font-size: var(--text-body);
  }

  .tabs button.active {
    border-bottom-color: var(--color-accent);
    color: var(--color-text);
    font-weight: var(--weight-medium);
  }

  .tabs button:hover:not(:disabled) {
    background: transparent;
    color: var(--color-text);
  }

  .tab-panel {
    padding-top: 4px;
  }

  .retention-fields {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
    margin-top: 16px;
  }

  .retention-fields label {
    display: grid;
    gap: 7px;
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
  }

  .retention-fields select {
    min-height: 36px;
    padding: 6px 9px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
  }

  .retention-fields small,
  .retention-note,
  .cleanup-preview {
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    font-weight: var(--weight-regular);
    line-height: 1.5;
  }

  .retention-note {
    margin: 15px 0 7px;
  }

  .cleanup-preview {
    margin: 0 0 12px;
    padding: 10px 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
  }

  .storage-actions {
    display: flex;
    gap: 6px;
  }

  .storage-actions button:last-child {
    background: var(--color-accent-solid);
    border-color: var(--color-accent-solid);
    color: var(--color-on-accent);
  }

  .storage-actions button:last-child:hover:not(:disabled) {
    filter: brightness(1.05);
  }

  .theme-control {
    display: grid;
    max-width: 360px;
    gap: 7px;
    margin-top: 18px;
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
  }

  .theme-control select {
    min-height: 38px;
    padding: 7px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
  }

  .theme-control small {
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    font-weight: var(--weight-regular);
  }

  @media (max-width: 640px) {
    .retention-fields {
      grid-template-columns: 1fr;
    }

    .settings {
      padding: 24px 18px;
    }

    .header {
      gap: 14px;
      margin-bottom: 20px;
    }

    .intro {
      max-width: 38ch;
    }

    .add-section {
      padding: 14px;
    }

    .add-section :global(form > div:first-child) {
      flex-direction: column;
    }

    .add-section :global(form input),
    .add-section :global(form button) {
      width: 100%;
    }
  }

  .add-section {
    padding: 16px 18px;
    margin-bottom: 24px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg-secondary);
  }

  .add-section :global(form) {
    width: 100%;
    margin-top: 12px;
    padding: 0;
    border-top: 0;
  }

  .add-section :global(form > div:first-child) {
    gap: 8px;
  }

  .add-section :global(input) {
    min-height: 36px;
    padding: 8px 10px;
    background: var(--color-bg);
  }

  .add-section :global(button) {
    min-height: 36px;
    padding-inline: 14px;
    background: var(--color-accent-solid);
    border-color: var(--color-accent-solid);
    color: var(--color-on-accent);
  }

  .add-section :global(button:hover:not(:disabled)) {
    filter: brightness(1.05);
  }

  .section-heading {
    margin-bottom: 14px;
  }

  .feed-list {
    display: grid;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .feed-card {
    padding: 15px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
  }

  .feed-info {
    min-width: 0;
    flex: 1;
  }

  .feed-card h3 {
    margin-bottom: 3px;
    font-size: var(--text-title);
    font-weight: var(--weight-medium);
  }

  .url {
    margin-bottom: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    overflow-wrap: anywhere;
  }

  .identity {
    margin-bottom: 10px;
  }

  .meta-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 12px;
  }

  .meta-pill {
    padding: 2px 9px;
    border-radius: var(--radius-pill);
    background: var(--color-hover-bg);
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
    font-weight: var(--weight-medium);
    line-height: 1.6;
  }

  .meta-pill-accent {
    background: var(--color-selection-bg);
    color: var(--color-accent);
  }

  .organization-controls {
    display: flex;
    align-items: flex-end;
    flex-wrap: wrap;
    gap: 8px;
    padding-top: 12px;
    border-top: 1px solid var(--color-border);
  }
  .setting-control {
    display: grid;
    gap: 4px;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }
  .setting-control select {
    min-height: 30px;
    min-width: 130px;
    padding: 4px 7px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    cursor: pointer;
  }
  .visibility-control {
    display: inline-flex;
    min-height: 30px;
    align-items: center;
    gap: 8px;
    padding: 4px 2px;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    cursor: pointer;
  }
  .visibility-control input {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
  }
  .switch-track {
    display: inline-flex;
    width: 32px;
    height: 18px;
    align-items: center;
    padding: 2px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-pill);
    background: var(--color-hover-bg);
    transition:
      background-color 120ms ease,
      border-color 120ms ease;
  }
  .switch-track > span {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--color-text-tertiary);
    transition:
      transform 120ms ease,
      background-color 120ms ease;
  }
  .visibility-control input:checked + .switch-track {
    border-color: var(--color-accent);
    background: var(--color-accent);
  }
  .visibility-control input:checked + .switch-track > span {
    transform: translateX(14px);
    background: white;
  }
  .visibility-control input:focus-visible + .switch-track {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .visibility-control:has(input:disabled) {
    opacity: 0.55;
    cursor: default;
  }
  .visibility-label {
    color: var(--color-text-secondary);
  }
  .visibility-control input:checked ~ .visibility-label {
    color: var(--color-accent);
  }

  .feed-error {
    margin-top: 9px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    flex-shrink: 0;
    gap: 6px;
  }

  .unsubscribe {
    color: var(--color-error);
  }

  :global(.confirm-row) {
    margin-top: 10px;
  }

  @media (max-width: 640px) {
    .feed-card {
      flex-direction: column;
    }

    .actions {
      align-self: flex-end;
    }
  }
</style>
