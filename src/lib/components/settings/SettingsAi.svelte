<script lang="ts">
  import { aiAutoOpen, setAiAutoOpen } from '$lib/ai/preferences.svelte';
  import { onMount } from 'svelte';
  import Spinner from '../Spinner.svelte';
  import SettingsGroup from './SettingsGroup.svelte';
  import SettingsGrid from './SettingsGrid.svelte';
  import SettingsRow from './SettingsRow.svelte';
  import ErrorCard from '../ErrorCard.svelte';
  import { checkAiProviders } from '$lib/api';
  import { t } from '$lib/i18n/index.svelte';
  import type { AiProvider, AiProviderStatus } from '$lib/types';

  // Shared with the AI panel's provider select (AiPanel.svelte).
  const PROVIDER_KEY = 'aequo:aiProvider';
  const NAMES: Record<AiProvider, string> = { codex: 'Codex', claude: 'Claude Code' };
  const PROVIDERS: AiProvider[] = ['codex', 'claude'];

  function readProvider(): AiProvider {
    try {
      return localStorage.getItem(PROVIDER_KEY) === 'claude' ? 'claude' : 'codex';
    } catch {
      return 'codex';
    }
  }

  let provider = $state<AiProvider>(readProvider());
  let saveStatus = $state<{ text: string; error: boolean } | null>(null);
  let providerStatusKey = $state(0);
  let autoStatus = $state<{ text: string; error: boolean } | null>(null);
  let autoStatusKey = $state(0);
  let statuses = $state<AiProviderStatus[] | null>(null);
  let checking = $state(false);
  let checkError = $state<string | null>(null);

  function updateProvider(next: AiProvider) {
    const previous = provider;
    provider = next;
    providerStatusKey++;
    try {
      localStorage.setItem(PROVIDER_KEY, next);
      saveStatus = { text: t('settings.saved'), error: false };
    } catch (error) {
      provider = previous;
      saveStatus = { text: t('settings.saveFailed', { error: String(error) }), error: true };
    }
  }

  async function check() {
    checking = true;
    checkError = null;
    try {
      statuses = await checkAiProviders();
    } catch (error) {
      // Without this, a rejection left every provider row showing a
      // permanent spinner (`checking || !status`) with no way out.
      checkError = String(error);
    } finally {
      checking = false;
    }
  }

  onMount(check);
</script>

<SettingsGrid>
  <SettingsGroup title={t('settings.group.assistant')}>
    <SettingsRow
      label={t('settings.ai.provider')}
      for="settings-ai-provider"
      hint={t('settings.ai.providerHint')}
      status={saveStatus?.text}
      statusTone={saveStatus?.error ? 'error' : 'neutral'}
      statusKey={providerStatusKey}
    >
      <select
        id="settings-ai-provider"
        value={provider}
        onchange={(event) => updateProvider(event.currentTarget.value as AiProvider)}
      >
        {#each PROVIDERS as name (name)}<option value={name}>{NAMES[name]}</option>{/each}
      </select>
    </SettingsRow>
    <SettingsRow
      label={t('settings.ai.autoOpen')}
      for="ai-auto-open"
      hint={t('settings.ai.autoOpenHint')}
      status={autoStatus?.text}
      statusTone={autoStatus?.error ? 'error' : 'neutral'}
      statusKey={autoStatusKey}
    >
      <input
        id="ai-auto-open"
        type="checkbox"
        role="switch"
        class="ui-switch"
        aria-label={t('settings.ai.autoOpen')}
        checked={aiAutoOpen()}
        onchange={(event) => {
          try {
            setAiAutoOpen(event.currentTarget.checked);
            autoStatus = { text: t('settings.saved'), error: false };
          } catch (error) {
            event.currentTarget.checked = aiAutoOpen();
            autoStatus = { text: t('settings.saveFailed', { error: String(error) }), error: true };
          }
          autoStatusKey++;
        }}
      />
    </SettingsRow>
  </SettingsGroup>

  <SettingsGroup title={t('settings.ai.cli')}>
    {#snippet action()}
      <button
        type="button"
        class="secondary ui-action ui-action--secondary"
        disabled={checking}
        onclick={check}
      >
        {checking ? t('settings.ai.checking') : t('settings.ai.check')}
      </button>
    {/snippet}
    <ul aria-busy={checking}>
      {#each PROVIDERS as name (name)}
        {@const status = statuses?.find((entry) => entry.provider === name)}
        <li>
          <SettingsRow label={NAMES[name]}>
            {#if checking}
              <span class="state pending"><Spinner size={12} /></span>
            {:else if !status}
              <!-- The whole check failed (checkError below) rather than this
               one provider - say nothing here instead of spinning forever,
               since the ErrorCard already explains the failure once. -->
            {:else if !status.installed}
              <span class="state off"><span class="dot"></span>{t('settings.ai.notFound')}</span>
            {:else if status.error}
              <span class="state off"
                ><span class="dot"></span>{t('settings.ai.failed', { error: status.error })}</span
              >
            {:else}
              <span class="state on"><span class="dot"></span>{t('settings.ai.installed')}</span>
              {#if status.version}<span class="version">{status.version}</span>{/if}
            {/if}
          </SettingsRow>
        </li>
      {/each}
    </ul>
    {#if checkError}
      <ErrorCard
        title={t('settings.ai.checkFailed.title')}
        body={checkError}
        onRetry={check}
        retrying={checking}
      />
    {/if}
  </SettingsGroup>
</SettingsGrid>

<div class="notes">
  <p>{t('settings.ai.loginNote')}</p>
  <p>{t('settings.ai.runsLocal')}</p>
  <p>{t('settings.ai.historyNote')}</p>
</div>

<style>
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: block;
  }

  li + li {
    border-top: 1px solid var(--color-border);
  }

  .state {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-small);
    font-weight: var(--weight-medium);
    /* Shared fade-in keyframe (+layout.svelte) - see SettingsRow.svelte. */
    animation: fade-in var(--motion-fast) var(--ease-enter) both;
  }

  .dot {
    width: 7px;
    height: 7px;
    flex-shrink: 0;
    border-radius: 50%;
    background: currentColor;
  }

  .state.on {
    color: var(--color-success);
  }

  .state.on .dot {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-success) 20%, transparent);
  }

  .state.off {
    color: var(--color-error);
  }

  .state.pending {
    color: var(--color-text-tertiary);
  }

  .version {
    margin-left: auto;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    font-variant-numeric: tabular-nums;
  }

  .notes {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: var(--space-3) 0 0;
  }

  .notes p {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }
</style>
