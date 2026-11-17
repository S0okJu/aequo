<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import FeedSettings from '$lib/components/FeedSettings.svelte';
  import { listFeedFolders, listFeeds } from '$lib/api';
  import { showToast } from '$lib/toast.svelte.ts';
  import { t } from '$lib/i18n/index.svelte';
  import type { FeedFolder, FeedSummary } from '$lib/types';

  let feeds = $state<FeedSummary[]>([]);
  let folders = $state<FeedFolder[]>([]);
  let initialTab = $state<'feeds' | 'folders' | 'storage' | 'appearance'>('feeds');
  let ready = $state(false);
  let loadError = $state<string | null>(null);

  async function refresh() {
    try {
      const [nextFeeds, nextFolders] = await Promise.all([listFeeds(), listFeedFolders()]);
      feeds = nextFeeds;
      folders = nextFolders;
      loadError = null;
    } catch (error) {
      loadError = String(error);
    }
  }

  onMount(async () => {
    const tab = new URLSearchParams(window.location.search).get('tab');
    if (tab === 'folders' || tab === 'storage' || tab === 'feeds' || tab === 'appearance') {
      initialTab = tab;
    }
    await refresh();
    ready = true;
  });

  async function handleFeedAdded() {
    await refresh();
    showToast(t('toast.feedAdded'), { type: 'success' });
  }
</script>

<main class="settings-page">
  {#if ready}
    {#if loadError}
      <p class="load-error" role="alert">Could not load settings: {loadError}</p>
    {/if}
    <FeedSettings
      {feeds}
      {folders}
      {initialTab}
      onAdded={handleFeedAdded}
      onChanged={refresh}
      onBack={() => goto(resolve('/'))}
    />
  {/if}
</main>

<style>
  .settings-page {
    background: var(--color-bg-secondary);
  }

  .load-error {
    max-width: 920px;
    margin: 0 auto;
    padding: 16px 48px 0;
    color: var(--color-error);
  }
</style>
