<script lang="ts">
  import { onMount } from 'svelte';
  import SettingsRow from './SettingsRow.svelte';
  import { checkAiProviders } from '$lib/api';
  import { t } from '$lib/i18n/index.svelte';
  import type { AiProvider, AiProviderStatus } from '$lib/types';

  // Shared with the AI panel's provider select (AiPanel.svelte).
  const PROVIDER_KEY = 'aequo:aiProvider';
  const NAMES: Record<AiProvider, string> = { codex: 'Codex', claude: 'Claude Code' };

  function readProvider(): AiProvider {
    try {
      return localStorage.getItem(PROVIDER_KEY) === 'claude' ? 'claude' : 'codex';
    } catch {
      return 'codex';
    }
  }

  let provider = $state<AiProvider>(readProvider());
  let saved = $state(false);
  let statuses = $state<AiProviderStatus[] | null>(null);
  let checking = $state(false);

  function updateProvider(next: AiProvider) {
    provider = next;
    try {
      localStorage.setItem(PROVIDER_KEY, next);
      saved = true;
    } catch {
      saved = false;
    }
  }

  async function check() {
    checking = true;
    try {
      statuses = await checkAiProviders();
    } finally {
      checking = false;
    }
  }

  onMount(check);
</script>

<SettingsRow
  label={t('settings.ai.provider')}
  for="setting-ai-provider"
  hint={t('settings.ai.providerHint')}
  status={saved ? t('settings.saved') : null}
>
  <select
    id="setting-ai-provider"
    value={provider}
    onchange={(event) => updateProvider(event.currentTarget.value as AiProvider)}
  >
    <option value="codex">{NAMES.codex}</option>
    <option value="claude">{NAMES.claude}</option>
  </select>
</SettingsRow>

<section class="cli" aria-labelledby="cli-heading">
  <div class="cli-heading">
    <h2 id="cli-heading">{t('settings.ai.cli')}</h2>
    <button type="button" class="secondary" disabled={checking} onclick={check}>
      {checking ? t('settings.ai.checking') : t('settings.ai.check')}
    </button>
  </div>
  <ul>
    {#each statuses ?? [] as status (status.provider)}
      <li>
        <span class="name">{NAMES[status.provider]}</span>
        {#if !status.installed}
          <span class="state off">{t('settings.ai.notFound')}</span>
        {:else if status.error}
          <span class="state off">{t('settings.ai.failed', { error: status.error })}</span>
        {:else}
          <span class="state on">{t('settings.ai.installed')}</span>
          {#if status.version}<span class="version">{status.version}</span>{/if}
        {/if}
      </li>
    {/each}
  </ul>
  <p class="note">{t('settings.ai.loginNote')}</p>
</section>

<p class="note">{t('settings.ai.runsLocal')}</p>
<p class="note">{t('settings.ai.historyNote')}</p>

<style>
  .cli {
    padding: var(--space-4) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .cli-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  h2 {
    margin: 0;
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: var(--space-3) 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-body);
  }

  .name {
    min-width: 110px;
  }

  .state {
    font-size: var(--text-small);
    font-weight: var(--weight-medium);
  }

  .state.on {
    color: var(--color-success);
  }

  .state.off {
    color: var(--color-error);
  }

  .version {
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .note {
    margin: var(--space-3) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }

  .secondary {
    padding: 4px var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  .secondary:hover:not(:disabled) {
    background: var(--color-hover-bg);
  }
</style>
