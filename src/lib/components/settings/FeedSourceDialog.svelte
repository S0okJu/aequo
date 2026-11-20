<script lang="ts">
  import IconButton from '../IconButton.svelte';
  import Spinner from '../Spinner.svelte';
  import { getFeedRawContent } from '$lib/api';
  import { formatDateTime } from '$lib/format';
  import { t } from '$lib/i18n/index.svelte';
  import type { FeedRawContent } from '$lib/types';

  interface Props {
    open: boolean;
    feedId: number | null;
    feedName: string;
    onClose: () => void;
  }

  let { open, feedId, feedName, onClose }: Props = $props();

  let dialog = $state<HTMLDialogElement | undefined>(undefined);
  let loadedFor = $state<number | null>(null);
  let raw = $state<FeedRawContent | null>(null);
  let loadError = $state<string | null>(null);
  let copied = $state(false);

  // A native modal <dialog> gives focus trapping, Escape-to-close and focus
  // return to the opener for free (docs/UI_SIMPLIFICATION_PLAN.md §3.11).
  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });

  $effect(() => {
    if (!open || feedId === null || loadedFor === feedId) return;
    const requested = feedId;
    raw = null;
    loadError = null;
    copied = false;
    getFeedRawContent(requested)
      .then((content) => {
        if (loadedFor === requested) return; // superseded by a newer request
        raw = content;
        loadedFor = requested;
      })
      .catch((error) => {
        loadError = String(error);
        loadedFor = requested;
      });
  });

  // Forget the last load once the dialog closes, so reopening on the same
  // feed re-fetches rather than showing a stale snapshot.
  $effect(() => {
    if (!open) {
      loadedFor = null;
      raw = null;
      loadError = null;
    }
  });

  async function copyContent() {
    if (!raw) return;
    try {
      await navigator.clipboard.writeText(raw.content);
      copied = true;
    } catch {
      copied = false;
    }
  }
</script>

<dialog
  bind:this={dialog}
  class="feed-source-dialog"
  aria-labelledby="feed-source-title"
  onclose={onClose}
  onclick={(event) => {
    if (event.target === dialog) onClose();
  }}
>
  {#if open}
    <div class="panel">
      <header>
        <h2 id="feed-source-title">{t('subs.sourceTitle', { name: feedName })}</h2>
        <IconButton icon="close" label={t('common.close')} onclick={onClose} />
      </header>

      {#if loadedFor !== feedId}
        <div class="state">
          <Spinner size={18} />
        </div>
      {:else if loadError}
        <p class="state error">{loadError}</p>
      {:else if !raw}
        <p class="state">{t('subs.sourceEmpty')}</p>
      {:else}
        <div class="meta">
          <span>{t('subs.sourceFetchedAt', { time: formatDateTime(raw.fetchedAt) })}</span>
          <button type="button" class="ui-action small ui-action--secondary" onclick={copyContent}>
            {copied ? t('subs.copied') : t('subs.sourceCopy')}
          </button>
        </div>
        <pre class="source"><code>{raw.content}</code></pre>
      {/if}
    </div>
  {/if}
</dialog>

<style>
  .feed-source-dialog {
    width: min(720px, calc(100vw - 32px));
    height: min(560px, calc(100vh - 64px));
    padding: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-xl);
    background: var(--color-bg);
    color: var(--color-text);
    box-shadow: var(--shadow-popover);
  }

  .feed-source-dialog::backdrop {
    background: rgba(0, 0, 0, 0.25);
  }

  .panel {
    display: flex;
    height: 100%;
    flex-direction: column;
    padding: var(--space-4) var(--space-4) var(--space-4) var(--space-5);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  h2 {
    min-width: 0;
    flex: 1;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    padding: var(--space-5);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .state.error {
    color: var(--color-error);
  }

  .meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-3) 0 var(--space-2);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .source {
    flex: 1;
    margin: 0;
    overflow: auto;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }

  .source code {
    font-family: ui-monospace, 'SF Mono', Menlo, monospace;
    white-space: pre;
  }
</style>
