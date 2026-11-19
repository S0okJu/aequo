<script lang="ts">
  import { onMount } from 'svelte';
  import SettingsGroup from './SettingsGroup.svelte';
  import SettingsRow from './SettingsRow.svelte';
  import { currentLocale, t, type MessageKey } from '$lib/i18n/index.svelte';
  import { refreshUpdates, updateAction, updateState } from '$lib/updates.svelte';

  const snapshot = $derived(updateState.snapshot);
  const busy = $derived(
    updateState.pending ||
      (snapshot &&
        ['checking', 'waiting_for_idle', 'downloading', 'verifying', 'installing'].includes(
          snapshot.phase,
        )),
  );
  const code = $derived(updateState.error ?? snapshot?.error?.code);
  const errors: Record<string, MessageKey> = {
    network: 'updates.error.network',
    http: 'updates.error.network',
    metadata: 'updates.error.metadata',
    signature: 'updates.error.signature',
    too_large: 'updates.error.tooLarge',
    storage: 'updates.error.storage',
    unavailable: 'updates.unavailable',
    rate_limited: 'updates.error.rateLimited',
    disabled: 'updates.error.disabled',
    work_active: 'updates.error.workActive',
    draft_active: 'updates.error.draftActive',
    install_location: 'updates.error.location',
    permission: 'updates.error.permission',
    install_failed: 'updates.error.install',
    restore_failed: 'updates.error.restore',
    not_applied: 'updates.error.notApplied',
    check_required: 'updates.error.checkRequired',
  };
  const progress = $derived(
    snapshot?.totalBytes
      ? Math.min(100, Math.round((snapshot.downloadedBytes / snapshot.totalBytes) * 100))
      : null,
  );
  let later = $state(false);
  function date(value: number | string): string {
    const parsed = new Date(typeof value === 'number' ? value * 1000 : value);
    return Number.isNaN(parsed.valueOf()) ? '' : parsed.toLocaleString(currentLocale());
  }
  onMount(() => {
    void refreshUpdates();
  });
</script>

<SettingsGroup title={t('updates.title')}>
  <SettingsRow label={t('updates.current')}>
    <span>{snapshot?.currentVersion ?? '—'}</span>
  </SettingsRow>
  <SettingsRow label={t('updates.auto')} for="automatic-updates" hint={t('updates.autoHint')}>
    <input
      id="automatic-updates"
      type="checkbox"
      role="switch"
      class="ui-switch"
      checked={snapshot?.autoUpdate ?? true}
      disabled={!snapshot || updateState.pending}
      onchange={(event) =>
        updateAction('set_update_settings', { autoUpdate: event.currentTarget.checked })}
    />
  </SettingsRow>
  <SettingsRow
    label={t('updates.status')}
    hint={snapshot?.lastSuccessfulCheckAt
      ? t('updates.checked', { date: date(snapshot.lastSuccessfulCheckAt) })
      : t('updates.neverChecked')}
  >
    <div class="actions">
      <span role="status" aria-live="polite"
        >{snapshot
          ? t(`updates.phase.${snapshot.phase}` as MessageKey)
          : t('updates.phase.idle')}</span
      >
      <button
        type="button"
        class="ui-action ui-action--secondary"
        disabled={!snapshot?.enabled || !!busy || snapshot.phase === 'restart_required'}
        onclick={() => updateAction('check_for_updates')}>{t('updates.check')}</button
      >
    </div>
  </SettingsRow>
  {#if snapshot && !snapshot.enabled}<p class="hint">{t('updates.unavailable')}</p>{/if}
  {#if code}<p class="error" role="alert">{t(errors[code] ?? 'updates.error.install')}</p>{/if}
  {#if snapshot?.release}
    <div class="release">
      <p class="version">{t('updates.newVersion', { version: snapshot.release.version })}</p>
      {#if snapshot.release.publishedAt}<p class="hint">
          {date(snapshot.release.publishedAt)}
        </p>{/if}
      {#if snapshot.release.notes}<p class="notes">{snapshot.release.notes}</p>{/if}
      <!-- Official external release URL, validated by the native updater. -->
      <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
      <a href={snapshot.release.releaseUrl} target="_blank" rel="noreferrer"
        >{t('updates.releaseNotes')}</a
      >
      {#if ['available', 'error'].includes(snapshot.phase) && snapshot.error?.stage !== 'check'}
        <button
          type="button"
          class="ui-action ui-action--secondary"
          disabled={!!busy || !snapshot.enabled}
          onclick={() => updateAction('install_available_update')}>{t('updates.install')}</button
        >
      {/if}
    </div>
  {/if}
  {#if snapshot?.phase === 'downloading'}
    <div class="download">
      <progress aria-label={t('updates.downloadProgress')} max="100" value={progress ?? undefined}
      ></progress>
      <span
        >{Math.round(snapshot.downloadedBytes / 1024 / 1024)} MB{#if progress !== null}
          · {progress}%{/if}</span
      >
    </div>
  {/if}
  {#if snapshot?.phase === 'restart_required'}
    <div class="restart">
      <p role="status">
        {t('updates.installed', { version: snapshot.pendingInstalledVersion ?? '' })}
      </p>
      <div class="actions">
        <button
          type="button"
          class="ui-action ui-action--secondary"
          disabled={updateState.pending}
          onclick={() => updateAction('restart_after_update')}>{t('updates.restart')}</button
        >
        {#if !later}<button
            type="button"
            class="ui-action ui-action--secondary"
            onclick={() => (later = true)}>{t('updates.later')}</button
          >{/if}
      </div>
    </div>
  {/if}
</SettingsGroup>

<style>
  .actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .hint {
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }
  .error {
    color: var(--color-error);
    font-size: var(--text-small);
  }
  .release,
  .restart {
    padding: var(--space-3) 0;
  }
  .version {
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
  }
  .notes {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 220px;
    overflow: auto;
    font-size: var(--text-small);
  }
  a {
    display: inline-block;
    margin-right: var(--space-3);
    color: var(--color-accent);
    font-size: var(--text-small);
  }
  .download {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    font-size: var(--text-small);
  }
  progress {
    accent-color: var(--color-accent);
    max-width: 100%;
  }
  input {
    accent-color: var(--color-accent);
  }
</style>
