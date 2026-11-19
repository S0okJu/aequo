<script lang="ts">
  import { onMount } from 'svelte';
  import SegmentedControl from '../SegmentedControl.svelte';
  import SettingsGroup from './SettingsGroup.svelte';
  import SettingsGrid from './SettingsGrid.svelte';
  import SettingsRow from './SettingsRow.svelte';
  import StatePlaceholder from '../StatePlaceholder.svelte';
  import { delayed } from '$lib/pending.svelte';
  import { t, type MessageKey } from '$lib/i18n/index.svelte';
  import {
    resourceState,
    resourcePreset,
    refreshResourceSettings,
    saveResourceSettings,
  } from '$lib/resources.svelte';
  import type { ResourceMode, ResourceSettings } from '$lib/types';

  let draft = $state<ResourceSettings | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let feedback = $state<string | null>(null);
  let failed = $state(false);
  let generation = 0;
  let disposed = false;
  const showLoading = delayed(() => loading && !draft);

  const cpuFields = [
    { field: 'feedConcurrency', values: [1, 2, 3, 4], unit: 'count' },
    { field: 'articleConcurrency', values: [1, 2, 3], unit: 'count' },
    { field: 'minAutoRefreshSecs', values: [300, 900, 1800, 3600], unit: 'minutes' },
    { field: 'aiFlushIntervalMs', values: [33, 50, 100, 200], unit: 'ms' },
  ] as const;
  const memoryFields = [
    { field: 'retainedPages', values: [2, 3, 4, 5], unit: 'pages' },
    { field: 'cacheBudgetMib', values: [16, 32, 64], unit: 'mib' },
  ] as const;

  $effect(() => {
    const status = resourceState.status;
    if (status && !saving) draft = { ...status.snapshot.settings };
  });

  async function load() {
    loading = true;
    try {
      await refreshResourceSettings();
      if (!disposed) {
        failed = false;
        feedback = null;
      }
    } catch (error) {
      if (!disposed) {
        failed = true;
        feedback = t('performance.loadFailed', { error: String(error) });
      }
    } finally {
      if (!disposed) loading = false;
    }
  }

  onMount(() => {
    void load();
    return () => {
      disposed = true;
    };
  });

  function save(settings: ResourceSettings) {
    const ownGeneration = ++generation;
    draft = settings;
    saving = true;
    failed = false;
    feedback = t('performance.saving');
    void saveResourceSettings(settings)
      .then((status) => {
        if (disposed || ownGeneration !== generation) return;
        feedback = t(status.runningWork > 0 ? 'performance.nextWork' : 'performance.applied');
      })
      .catch((error) => {
        if (disposed || ownGeneration !== generation) return;
        failed = true;
        feedback = t('performance.saveFailed', { error: String(error) });
      })
      .finally(() => {
        if (!disposed && ownGeneration === generation) saving = false;
      });
  }

  function update<K extends keyof ResourceSettings>(field: K, value: ResourceSettings[K]) {
    if (draft) save({ ...draft, mode: 'custom', [field]: value });
  }

  function mode(value: ResourceMode) {
    if (value === 'custom') {
      if (draft) save({ ...draft, mode: value });
    } else save(resourcePreset(value));
  }

  function label(field: string): string {
    return t(`performance.field.${field}` as MessageKey);
  }
  function hint(field: string): string {
    return t(`performance.hint.${field}` as MessageKey);
  }
  function option(value: number, unit: string): string {
    return t(`performance.unit.${unit}` as MessageKey, {
      count: unit === 'minutes' ? value / 60 : value,
    });
  }
</script>

{#if draft}
  {#if feedback}
    <p class="feedback" class:error={failed} role={failed ? 'alert' : 'status'}>{feedback}</p>
  {/if}
  {#if resourceState.status?.recoveryError}
    <p class="feedback error" role="alert">{t('performance.recovery')}</p>
  {/if}
{/if}
{#if !draft && loading}
  <!-- Still within the first load (or a retry) - only show anything once
       showLoading's 200ms delay has passed, so a fast load never flashes a
       spinner. Critically, this is loading && !draft, not just !draft:
       without the `loading` check this would show the error branch below
       during that same 200ms window, before the request has even settled. -->
  {#if showLoading.value}
    <StatePlaceholder kind="loading" />
  {/if}
{:else if !draft}
  <!-- `feedback` is already the specific "couldn't load resource settings:
       {error}" sentence from load() - no separate generic title needed. -->
  <StatePlaceholder kind="error" body={feedback ?? ''}>
    {#snippet action()}
      <button type="button" class="secondary ui-action ui-action--secondary" onclick={load}
        >{t('performance.retry')}</button
      >
    {/snippet}
  </StatePlaceholder>
{:else}
  <fieldset disabled={loading || Boolean(resourceState.status?.recoveryError)}>
    <SettingsGrid>
      <SettingsGroup title={t('performance.mode')}>
        <SettingsRow
          label={t('performance.mode')}
          labelId="resource-mode"
          hint={t('performance.modeHint')}
        >
          <SegmentedControl
            size="sm"
            block
            labelledby="resource-mode"
            value={draft.mode}
            onChange={mode}
            options={(['light', 'balanced', 'fast', 'custom'] as ResourceMode[]).map((value) => ({
              value,
              label: t(`performance.mode.${value}` as MessageKey),
            }))}
          />
        </SettingsRow>
      </SettingsGroup>
      <SettingsGroup title={t('performance.cpu')}>
        {#each cpuFields as entry (entry.field)}
          <SettingsRow
            label={label(entry.field)}
            for={`resource-${entry.field}`}
            hint={hint(entry.field)}
          >
            <select
              id={`resource-${entry.field}`}
              value={draft[entry.field]}
              onchange={(event) => update(entry.field, Number(event.currentTarget.value))}
            >
              {#each entry.values as value (value)}<option {value}
                  >{option(value, entry.unit)}</option
                >{/each}
              {#if entry.field === 'aiFlushIntervalMs' && !entry.values.includes(draft.aiFlushIntervalMs as never)}
                <option value={draft.aiFlushIntervalMs}
                  >{option(draft.aiFlushIntervalMs, entry.unit)}</option
                >
              {/if}
            </select>
          </SettingsRow>
        {/each}
        <SettingsRow
          label={label('prefetchFullText')}
          for="resource-prefetch"
          hint={hint('prefetchFullText')}
        >
          <input
            id="resource-prefetch"
            type="checkbox"
            role="switch"
            class="ui-switch"
            checked={draft.prefetchFullText}
            onchange={(event) => update('prefetchFullText', event.currentTarget.checked)}
          />
        </SettingsRow>
        <SettingsRow
          label={label('pauseOptionalWhenHidden')}
          for="resource-hidden"
          hint={hint('pauseOptionalWhenHidden')}
        >
          <input
            id="resource-hidden"
            type="checkbox"
            role="switch"
            class="ui-switch"
            checked={draft.pauseOptionalWhenHidden}
            onchange={(event) => update('pauseOptionalWhenHidden', event.currentTarget.checked)}
          />
        </SettingsRow>
      </SettingsGroup>
      <SettingsGroup title={t('performance.memory')}>
        {#each memoryFields as entry (entry.field)}
          <SettingsRow
            label={label(entry.field)}
            for={`resource-${entry.field}`}
            hint={hint(entry.field)}
          >
            <select
              id={`resource-${entry.field}`}
              value={draft[entry.field]}
              onchange={(event) => update(entry.field, Number(event.currentTarget.value))}
            >
              {#each entry.values as value (value)}<option {value}
                  >{option(value, entry.unit)}</option
                >{/each}
            </select>
          </SettingsRow>
        {/each}
        <p class="note">{t('performance.budgetNote')}</p>
      </SettingsGroup>
    </SettingsGrid>
  </fieldset>
  <button
    type="button"
    class="secondary ui-action ui-action--secondary"
    disabled={saving || loading}
    onclick={() => save(resourcePreset('balanced'))}>{t('performance.restore')}</button
  >
{/if}

<style>
  fieldset {
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  .secondary {
    align-self: flex-start;
    margin-top: var(--space-3);
    cursor: pointer;
  }
  .feedback,
  .note {
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .error {
    color: var(--color-error);
  }
</style>
