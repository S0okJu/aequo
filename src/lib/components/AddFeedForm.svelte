<script lang="ts">
  import { addFeed, updateFeedOrganization } from '$lib/api';
  import ErrorCard from './ErrorCard.svelte';
  import { t } from '$lib/i18n/index.svelte';
  import type { FeedFolder } from '$lib/types';

  interface Props {
    onAdded: (feedId: number) => void;
    /**
     * Dialog layout: with `folders`, the form also picks the new feed's
     * folder and shows Cancel/Add actions under the fields.
     */
    folders?: FeedFolder[];
    defaultFolderId?: number | null;
    onCancel?: () => void;
  }

  let { onAdded, folders, defaultFolderId = null, onCancel }: Props = $props();

  // svelte-ignore state_referenced_locally
  let folderId = $state<number | null>(defaultFolderId);
  let url = $state('');
  let urlInput = $state<HTMLInputElement | undefined>(undefined);
  // T1: the URL itself looks wrong - shown right under the input, no retry
  // (fixing the typo is the "retry"). T2: adding it failed after a valid
  // URL was sent - a retryable card, since trying again can genuinely help
  // (docs/UNIFIED_UX_MOTION_PLAN.md §6.2).
  let fieldError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let submitting = $state(false);

  function validatedUrl(trimmed: string): string | null {
    try {
      const parsed = new URL(trimmed);
      return parsed.protocol === 'http:' || parsed.protocol === 'https:'
        ? null
        : t('addFeed.errorScheme');
    } catch {
      return t('addFeed.errorInvalid');
    }
  }

  function friendlyFeedError(err: unknown): string {
    const raw = String(err);
    if (raw.includes('UNIQUE constraint')) return t('addFeed.errorDuplicate');
    if (raw.toLowerCase().includes('timed out')) return t('addFeed.errorTimeout');
    if (raw.toLowerCase().includes('dns') || raw.toLowerCase().includes('could not resolve'))
      return t('addFeed.errorUnreachable');
    if (raw.toLowerCase().includes('not a supported feed')) return t('addFeed.errorUnsupported');
    return t('addFeed.errorGeneric');
  }

  async function submit() {
    const trimmed = url.trim();
    if (!trimmed || submitting) return;

    const validationError = validatedUrl(trimmed);
    if (validationError) {
      fieldError = validationError;
      error = null;
      urlInput?.focus();
      return;
    }

    fieldError = null;
    submitting = true;
    error = null;
    try {
      const feedId = await addFeed(trimmed);
      // Filing into a folder is best-effort: the feed exists either way, and
      // it can still be moved from the sidebar if this step fails.
      if (folders && folderId !== null) {
        await updateFeedOrganization(feedId, folderId, false).catch((err) =>
          console.error('update_feed_organization failed:', err),
        );
      }
      url = '';
      onAdded(feedId);
    } catch (err) {
      error = friendlyFeedError(err);
    } finally {
      submitting = false;
    }
  }

  function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    void submit();
  }
</script>

<!-- `novalidate`: a completely scheme-less value (e.g. "not a url") otherwise
     fails the input's native type="url" constraint and the browser blocks
     the submit event before it ever reaches handleSubmit, so validatedUrl()
     never runs and our own inline message never shows - just the browser's
     own validation bubble. Our check (http/https only, friendly copy) fully
     replaces the native one. -->
<form class="add-feed" class:dialog={folders !== undefined} onsubmit={handleSubmit} novalidate>
  {#if folders === undefined}
    <div class="add-feed-row">
      <input
        type="url"
        placeholder={t('addFeed.placeholder')}
        bind:value={url}
        bind:this={urlInput}
        disabled={submitting}
        aria-label={t('addFeed.urlLabel')}
        aria-invalid={fieldError ? 'true' : undefined}
      />
      <button type="submit" disabled={submitting || !url.trim()}>
        {submitting ? t('addFeed.submitting') : t('addFeed.submit')}
      </button>
    </div>
    {#if fieldError}
      <p class="field-error" role="alert">{fieldError}</p>
    {/if}
  {:else}
    <label class="field">
      <span>{t('addFeed.urlLabel')}</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        type="url"
        placeholder="https://"
        bind:value={url}
        bind:this={urlInput}
        disabled={submitting}
        aria-invalid={fieldError ? 'true' : undefined}
        autofocus
      />
      {#if fieldError}
        <p class="field-error" role="alert">{fieldError}</p>
      {/if}
    </label>
    <label class="field">
      <span>{t('addFeed.folder')}</span>
      <select
        value={folderId ?? ''}
        disabled={submitting}
        onchange={(event) =>
          (folderId = event.currentTarget.value === '' ? null : Number(event.currentTarget.value))}
      >
        <option value="">{t('sidebar.unfiled')}</option>
        {#each folders as folder (folder.id)}
          <option value={folder.id}>{folder.name}</option>
        {/each}
      </select>
    </label>
  {/if}
  <p class="collection-hint">{t('addFeed.collectionHint')}</p>
  {#if error}
    <ErrorCard
      title={t('addFeed.failedTitle')}
      body={error}
      onRetry={submit}
      retrying={submitting}
      onDismiss={() => (error = null)}
    />
  {/if}
  {#if folders !== undefined}
    <div class="actions">
      <button type="button" onclick={onCancel} disabled={submitting}>{t('common.cancel')}</button>
      <button type="submit" class="primary" disabled={submitting || !url.trim()}>
        {submitting ? t('addFeed.submitting') : t('addFeed.submit')}
      </button>
    </div>
  {/if}
</form>

<style>
  .add-feed {
    padding: 8px;
    border-top: 1px solid var(--color-border);
  }

  .add-feed-row {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  input {
    min-width: 0;
    flex: 1;
    width: 100%;
    padding: 6px 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
  }

  input:disabled {
    opacity: 0.5;
  }

  button {
    font: inherit;
    font-size: var(--text-body);
    padding: 6px 9px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: var(--color-hover-bg);
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .add-feed.dialog {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: 0;
    border: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .field-error {
    margin: 2px 0 0;
    color: var(--color-error);
    font-size: var(--text-small);
  }

  .collection-hint {
    margin: var(--space-1) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-1);
  }

  button.primary {
    border-color: var(--color-accent-solid);
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
  }

  button.primary:hover:not(:disabled) {
    background: var(--color-accent-solid);
    filter: brightness(0.95);
  }

  /* ErrorCard's own root element now carries this class, not something
     rendered directly here - :global() reaches past its own Svelte
     scoping, kept local to this form via the .add-feed ancestor. */
  .add-feed :global(.error-card) {
    margin-top: 8px;
  }
</style>
