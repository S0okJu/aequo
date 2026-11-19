<script lang="ts">
  import { onMount } from 'svelte';
  import SegmentedControl from '../SegmentedControl.svelte';
  import SettingsGroup from './SettingsGroup.svelte';
  import SettingsGrid from './SettingsGrid.svelte';
  import SettingsRow from './SettingsRow.svelte';
  import StatePlaceholder from '../StatePlaceholder.svelte';
  import {
    getRetentionSettings,
    previewRetentionCleanup,
    runRetentionCleanup,
    setRetentionSettings,
  } from '$lib/api';
  import { delayed } from '$lib/pending.svelte';
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
  let statusKey = $state(0);
  let confirming = $state(false);
  let cleaning = $state(false);
  let saving = $state(false);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  // Lock policy and cleanup controls until the saved policy has a matching preview.
  const showLoading = delayed(() => loading);

  async function load() {
    loading = true;
    loadError = null;
    status = null;
    try {
      [retention, preview] = await Promise.all([getRetentionSettings(), previewRetentionCleanup()]);
    } catch (error) {
      loadError = String(error);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  async function update(field: keyof RetentionSettings, days: number) {
    if (!retention || saving || cleaning) return;
    saving = true;
    confirming = false;
    const previous = retention[field];
    retention = { ...retention, [field]: days };
    let persisted = false;
    try {
      await setRetentionSettings(retention);
      persisted = true;
      preview = null;
      const nextPreview = await previewRetentionCleanup();
      preview = nextPreview;
      status = { field, text: t('settings.saved'), error: false };
      statusKey++;
    } catch (error) {
      if (!persisted) retention = { ...retention, [field]: previous };
      status = {
        field,
        text: t(persisted ? 'settings.storage.previewFailed' : 'settings.saveFailed', {
          error: String(error),
        }),
        error: true,
      };
      statusKey++;
    } finally {
      saving = false;
    }
  }

  async function cleanNow() {
    if (saving || cleaning || nothingToClean) return;
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
      showToast(t('settings.storage.cleanFailed', { error: String(error) }), { type: 'error' });
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

{#if showLoading.value}
  <StatePlaceholder kind="loading" />
{:else if loadError}
  <StatePlaceholder kind="error" title={t('state.error.loadFailed.title')} body={loadError}>
    {#snippet action()}
      <button type="button" class="secondary ui-action ui-action--secondary" onclick={load}>
        {t('state.error.loadFailed.action')}
      </button>
    {/snippet}
  </StatePlaceholder>
{:else if retention}
  {@const current = retention}
  <SettingsGrid>
    <SettingsGroup title={t('settings.group.retention')}>
      {#snippet action()}
        <span class="saving" role="status">{saving ? t('settings.saving') : ''}</span>
      {/snippet}
      <SettingsRow
        label={t('settings.storage.read')}
        labelId="retention-read"
        hint={t('settings.storage.readHint')}
        status={status?.field === 'readRetentionDays' ? status.text : null}
        statusTone={status?.error ? 'error' : 'neutral'}
        {statusKey}
      >
        <SegmentedControl
          size="sm"
          block
          disabled={saving || cleaning}
          labelledby="retention-read"
          value={current.readRetentionDays}
          onChange={(days) => update('readRetentionDays', days)}
          options={[0, 90, 180, 365].map((days) => ({ value: days, label: daysLabel(days) }))}
        />
      </SettingsRow>

      <SettingsRow
        label={t('settings.storage.fullText')}
        labelId="retention-full-text"
        hint={t('settings.storage.fullTextHint')}
        status={status?.field === 'fullTextRetentionDays' ? status.text : null}
        statusTone={status?.error ? 'error' : 'neutral'}
        {statusKey}
      >
        <SegmentedControl
          size="sm"
          block
          disabled={saving || cleaning}
          labelledby="retention-full-text"
          value={current.fullTextRetentionDays}
          onChange={(days) => update('fullTextRetentionDays', days)}
          options={[0, 30, 90, 180, 365].map((days) => ({ value: days, label: daysLabel(days) }))}
        />
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title={t('settings.group.cleanup')} tone="danger">
      <SettingsRow
        label={t('settings.storage.note')}
        hint={preview
          ? t('settings.storage.preview', {
              posts: preview.articlesToRemove,
              texts: preview.fullTextsToClear,
              size: formatBytes(preview.estimatedBytesToFree),
            })
          : undefined}
      >
        {#if !preview && !saving}
          <button type="button" class="ui-action ui-action--secondary" onclick={load}>
            {t('state.error.loadFailed.action')}
          </button>
        {/if}
        {#if !confirming || !preview}
          <button
            type="button"
            class="secondary ui-action ui-action--secondary"
            disabled={nothingToClean || saving || cleaning}
            onclick={() => (confirming = true)}>{t('settings.storage.cleanNow')}</button
          >
        {/if}
      </SettingsRow>
      {#if confirming && preview}
        <div class="confirm-row" role="group" aria-label={t('settings.storage.cleanNow')}>
          <span
            >{t('settings.storage.cleanConfirm', {
              posts: preview.articlesToRemove,
              texts: preview.fullTextsToClear,
            })}</span
          >
          <div class="confirm-actions">
            <button
              type="button"
              class="ui-action ui-action--secondary"
              disabled={cleaning}
              onclick={() => (confirming = false)}>{t('common.cancel')}</button
            >
            <button
              type="button"
              class="confirm-remove ui-action ui-action--secondary"
              disabled={cleaning}
              onclick={cleanNow}
            >
              {cleaning ? t('settings.storage.cleaning') : t('settings.storage.cleanNow')}
            </button>
          </div>
        </div>
      {/if}
    </SettingsGroup>
  </SettingsGrid>
{/if}

<style>
  .confirm-row {
    margin-bottom: var(--space-4);
    /* Shared fade-in keyframe (+layout.svelte) - see SettingsRow.svelte. */
    animation: fade-in var(--motion-fast) var(--ease-enter) both;
  }
  .confirm-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-border);
    color: var(--color-text);
    font-size: var(--text-body);
  }
  .confirm-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .confirm-remove {
    color: var(--color-error);
    border-color: color-mix(in srgb, var(--color-error) 35%, var(--color-border));
  }
  .saving {
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }
</style>
