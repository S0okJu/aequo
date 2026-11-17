<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { page } from '$app/state';
  import AddFeedDialog from '$lib/components/AddFeedDialog.svelte';
  import SettingsAi from '$lib/components/settings/SettingsAi.svelte';
  import SettingsGeneral from '$lib/components/settings/SettingsGeneral.svelte';
  import SettingsStorage from '$lib/components/settings/SettingsStorage.svelte';
  import SettingsSubscriptions from '$lib/components/settings/SettingsSubscriptions.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import { listFeedFolders, listFeeds } from '$lib/api';
  import { showToast } from '$lib/toast.svelte.ts';
  import { t, type MessageKey } from '$lib/i18n/index.svelte';
  import type { FeedFolder, FeedSummary } from '$lib/types';

  type Section = 'general' | 'subscriptions' | 'ai' | 'storage';
  const SECTIONS: Section[] = ['general', 'subscriptions', 'ai', 'storage'];
  // Links from before the sectioned layout (`?tab=`) still land somewhere sensible.
  const LEGACY_TABS: Record<string, Section> = {
    feeds: 'subscriptions',
    folders: 'subscriptions',
    storage: 'storage',
    appearance: 'general',
  };

  // The section lives in the URL (`?section=`), so direct links and
  // back/forward work (docs/UI_SIMPLIFICATION_PLAN.md §3.10).
  const section = $derived.by((): Section => {
    const params = page.url.searchParams;
    const requested = params.get('section') ?? '';
    if ((SECTIONS as string[]).includes(requested)) return requested as Section;
    return LEGACY_TABS[params.get('tab') ?? ''] ?? 'general';
  });

  let feeds = $state<FeedSummary[]>([]);
  let folders = $state<FeedFolder[]>([]);
  let loadError = $state<string | null>(null);
  let addFeedOpen = $state(false);
  let selectedFeedId = $state<number | null>(null);
  let heading = $state<HTMLHeadingElement | undefined>(undefined);

  async function refresh() {
    try {
      [feeds, folders] = await Promise.all([listFeeds(), listFeedFolders()]);
      loadError = null;
    } catch (error) {
      loadError = String(error);
    }
  }

  onMount(() => {
    const feedParam = Number(page.url.searchParams.get('feed'));
    if (Number.isInteger(feedParam) && feedParam > 0) selectedFeedId = feedParam;
    refresh();
  });

  function sectionHref(target: Section): string {
    return resolve(`/settings?section=${target}`);
  }

  async function openSection(event: MouseEvent, target: Section) {
    event.preventDefault();
    if (target === section) return;
    await goto(resolve(`/settings?section=${target}`), { keepFocus: true, noScroll: true });
    await tick();
    // Moving focus to the new section's heading tells screen readers where
    // they landed; the heading isn't otherwise in the tab order.
    heading?.focus();
  }

  function back() {
    goto(resolve('/'));
  }

  async function handleFeedAdded(feedId: number) {
    addFeedOpen = false;
    await refresh();
    selectedFeedId = feedId;
    showToast(t('toast.feedAdded'), { type: 'success' });
  }

  function handleKeydown(event: KeyboardEvent) {
    // Sections handle their own Escape first (inline editors, the detail
    // panel); anything left over leaves settings.
    if (event.key !== 'Escape' || event.defaultPrevented || addFeedOpen) return;
    const active = document.activeElement;
    if (active && ['INPUT', 'TEXTAREA', 'SELECT'].includes(active.tagName)) return;
    back();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="settings-layout">
  <nav class="settings-nav" aria-label={t('settings.nav')}>
    <p class="nav-title">{t('settings.title')}</p>
    <button type="button" class="back" onclick={back}>
      <Icon name="chevron-left" size={14} />{t('settings.back')}
    </button>
    <ul>
      {#each SECTIONS as item (item)}
        <li>
          <!-- eslint-disable svelte/no-navigation-without-resolve -->
          <a
            href={sectionHref(item)}
            aria-current={section === item ? 'page' : undefined}
            onclick={(event) => openSection(event, item)}
            >{t(`settings.section.${item}` as MessageKey)}</a
          >
          <!-- eslint-enable svelte/no-navigation-without-resolve -->
        </li>
      {/each}
    </ul>
  </nav>

  <main class="settings-content" class:wide={section === 'subscriptions'}>
    <h1 bind:this={heading} tabindex="-1">{t(`settings.section.${section}` as MessageKey)}</h1>
    {#if loadError}
      <p class="load-error" role="alert">{t('settings.loadError', { error: loadError })}</p>
    {/if}
    {#if section === 'general'}
      <SettingsGeneral />
    {:else if section === 'subscriptions'}
      <SettingsSubscriptions
        {feeds}
        {folders}
        bind:selectedId={selectedFeedId}
        onChanged={refresh}
        onAddFeed={() => (addFeedOpen = true)}
      />
    {:else if section === 'ai'}
      <SettingsAi />
    {:else}
      <SettingsStorage onCleaned={refresh} />
    {/if}
  </main>
</div>

<AddFeedDialog
  open={addFeedOpen}
  {folders}
  onAdded={handleFeedAdded}
  onClose={() => (addFeedOpen = false)}
/>

<style>
  .settings-layout {
    display: grid;
    height: 100vh;
    height: 100dvh;
    grid-template-columns: 220px minmax(0, 1fr);
    background: var(--color-bg);
  }

  .settings-nav {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    overflow-y: auto;
    padding: var(--space-5) var(--space-2);
    border-right: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
  }

  .nav-title {
    margin: 0 var(--space-2);
    color: var(--color-text);
    font-size: var(--text-heading);
    font-weight: var(--weight-bold);
  }

  .back {
    display: inline-flex;
    align-self: flex-start;
    align-items: center;
    gap: var(--space-1);
    margin-bottom: var(--space-4);
    padding: 4px var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  .back:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  ul a {
    display: flex;
    min-height: 32px;
    align-items: center;
    padding: 6px var(--space-2);
    border-radius: var(--radius-sm);
    color: var(--color-text);
    font-size: var(--text-body);
    text-decoration: none;
  }

  ul a:hover {
    background: var(--color-hover-bg);
  }

  ul a[aria-current='page'] {
    background: var(--color-selection-bg);
    font-weight: var(--weight-medium);
  }

  .settings-content {
    display: flex;
    min-height: 0;
    flex-direction: column;
    overflow-y: auto;
    width: 100%;
    max-width: 760px;
    padding: var(--space-5) var(--space-6);
  }

  /* The subscriptions list + detail needs the room and scrolls internally. */
  .settings-content.wide {
    max-width: 1120px;
    overflow: hidden;
  }

  h1 {
    margin: 0 0 var(--space-4);
    font-size: var(--text-display);
    font-weight: var(--weight-bold);
  }

  h1:focus {
    outline: none;
  }

  .load-error {
    color: var(--color-error);
    font-size: var(--text-small);
  }
</style>
