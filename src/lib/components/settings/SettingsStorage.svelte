<script lang="ts">
  import { onMount } from 'svelte';
  import SettingsRow from './SettingsRow.svelte';
  import {
    getRetentionSettings,
    previewRetentionCleanup,
    runRetentionCleanup,
    setRetentionSettings,
  } from '$lib/api';
  import { showToast } from '$lib/toast.svelte.ts';
  import { t } from '$lib/i18n/index.svelte';
  import type { CleanupPreview, RetentionSettings } from '$lib/types';

  interface Props {
    /** Called after a cleanup removed posts, so counts elsewhere refresh. */
    onCleaned?: () => void;
  }

  let { onCleaned }: Props = $props();

  let retention = $state<RetentionSettings | null>(null);
  let preview = $state<CleanupPreview | null>(null);
  let status = $state<{ field: keyof RetentionSettings; text: string; error: boolean } | null>(
    null,
  );
  let confirming = $state(false);
  let cleaning = $state(false);
  let loadError = $state<string | null>(null);
  // Changes save immediately; a later change supersedes an in-flight save.
  let saveGeneration = 0;

  onMount(async () => {
    try {
      [retention, preview] = await Promise.all([getRetentionSettings(), previewRetentionCleanup()]);
    } catch (error) {
      loadError = String(error);
    }
  });

  async function update(field: keyof RetentionSettings, days: number) {
    if (!retention) return;
    const previous = retention[field];
    retention = { ...retention, [field]: days };
    const generation = ++saveGeneration;
    try {
      await setRetentionSettings(retention);
      const nextPreview = await previewRetentionCleanup();
      if (generation !== saveGeneration) return;
      preview = nextPreview;
      status = { field, text: t('settings.saved'), error: false };
    } catch (error) {
      if (generation !== saveGeneration) return;
      retention = { ...retention, [field]: previous };
      status = { field, text: t('settings.saveFailed', { error: String(error) }), error: true };
    }
  }

  async function cleanNow() {
    cleaning = true;
    try {
      const result = await runRetentionCleanup();
      preview = await previewRetentionCleanup();
      confirming = false;
      showToast(
        t('settings.storage.cleaned', {
          posts: result.articlesRemoved,
          texts: result.fullTextsCleared,
          size: formatBytes(result.bytesFreed),
        }),
        { type: 'success' },
      );
      onCleaned?.();
    } catch (error) {
      showToast(String(error), { type: 'error' });
    } finally {
      cleaning = false;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function daysLabel(days: number): string {
    return days === 0 ? t('settings.storage.forever') : t('settings.storage.days', { count: days });
  }

  const nothingToClean = $derived(
    !preview || (preview.articlesToRemove === 0 && preview.fullTextsToClear === 0),
  );
</script>

{#if loadError}
  <p class="load-error" role="alert">{t('settings.loadError', { error: loadError })}</p>
{:else if retention}
  <SettingsRow
    label={t('settings.storage.read')}
    for="retention-read"
    hint={t('settings.storage.readHint')}
    status={status?.field === 'readRetentionDays' ? status.text : null}
    statusTone={status?.error ? 'error' : 'neutral'}
  >
    <select
      id="retention-read"
      value={retention.readRetentionDays}
      onchange={(event) => update('readRetentionDays', Number(event.currentTarget.value))}
    >
      {#each [0, 90, 180, 365] as days (days)}
        <option value={days}>{daysLabel(days)}</option>
      {/each}
    </select>
  </SettingsRow>

  <SettingsRow
    label={t('settings.storage.fullText')}
    for="retention-full-text"
    hint={t('settings.storage.fullTextHint')}
    status={status?.field === 'fullTextRetentionDays' ? status.text : null}
    statusTone={status?.error ? 'error' : 'neutral'}
  >
    <select
      id="retention-full-text"
      value={retention.fullTextRetentionDays}
      onchange={(event) => update('fullTextRetentionDays', Number(event.currentTarget.value))}
    >
      {#each [0, 30, 90, 180, 365] as days (days)}
        <option value={days}>{daysLabel(days)}</option>
      {/each}
    </select>
  </SettingsRow>

  <p class="note">{t('settings.storage.note')}</p>

  <div class="cleanup">
    {#if preview}
      <p class="preview">
        {t('settings.storage.preview', {
          posts: preview.articlesToRemove,
          texts: preview.fullTextsToClear,
          size: formatBytes(preview.estimatedBytesToFree),
        })}
      </p>
    {/if}
    {#if confirming && preview}
      <div class="confirm-row">
        <span
          >{t('settings.storage.cleanConfirm', {
            posts: preview.articlesToRemove,
            texts: preview.fullTextsToClear,
          })}</span
        >
        <div class="confirm-actions">
          <button type="button" onclick={() => (confirming = false)}>{t('common.cancel')}</button>
          <button type="button" class="confirm-remove" disabled={cleaning} onclick={cleanNow}>
            {cleaning ? t('settings.storage.cleaning') : t('settings.storage.cleanNow')}
          </button>
        </div>
      </div>
    {:else}
      <button
        type="button"
        class="secondary"
        disabled={nothingToClean}
        onclick={() => (confirming = true)}>{t('settings.storage.cleanNow')}</button
      >
    {/if}
  </div>
{/if}

<style>
  .note {
    margin: var(--space-4) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .cleanup {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    margin-top: var(--space-5);
  }

  .preview {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .cleanup .confirm-row {
    align-self: stretch;
  }

  .secondary {
    padding: 6px var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    cursor: pointer;
  }

  .secondary:hover:not(:disabled) {
    background: var(--color-hover-bg);
  }

  .secondary:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .load-error {
    color: var(--color-error);
    font-size: var(--text-small);
  }
</style>
