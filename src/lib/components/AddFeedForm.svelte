<script lang="ts">
  import { addFeed } from '$lib/api';
  import Icon from '$lib/icons/Icon.svelte';

  interface Props {
    onAdded: () => void;
  }

  let { onAdded }: Props = $props();

  let url = $state('');
  let error = $state<string | null>(null);
  let submitting = $state(false);

  function validatedUrl(trimmed: string): string | null {
    try {
      const parsed = new URL(trimmed);
      return parsed.protocol === 'http:' || parsed.protocol === 'https:'
        ? null
        : 'Feed URL must start with http:// or https://';
    } catch {
      return "That doesn't look like a valid URL";
    }
  }

  function friendlyFeedError(err: unknown): string {
    const raw = String(err);
    if (raw.includes('UNIQUE constraint')) return "You're already subscribed to this feed";
    if (raw.toLowerCase().includes('timed out'))
      return 'The request timed out - the site may be slow or unreachable';
    if (raw.toLowerCase().includes('dns') || raw.toLowerCase().includes('could not resolve'))
      return "Couldn't reach that address - check the URL and try again";
    if (raw.toLowerCase().includes('not a supported feed'))
      return "That page doesn't look like a supported feed";
    return 'Something went wrong adding that feed. Please try again.';
  }

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    const trimmed = url.trim();
    if (!trimmed || submitting) return;

    const validationError = validatedUrl(trimmed);
    if (validationError) {
      error = validationError;
      return;
    }

    submitting = true;
    error = null;
    try {
      await addFeed(trimmed);
      url = '';
      onAdded();
    } catch (err) {
      error = friendlyFeedError(err);
    } finally {
      submitting = false;
    }
  }
</script>

<form class="add-feed" onsubmit={handleSubmit}>
  <div class="add-feed-row">
    <input
      type="url"
      placeholder="Add feed URL…"
      bind:value={url}
      disabled={submitting}
      aria-label="Feed URL"
    />
    <button type="submit" disabled={submitting || !url.trim()}>
      {submitting ? 'Adding…' : 'Add feed'}
    </button>
  </div>
  {#if error}
    <div class="error-card" role="alert">
      <span class="error-mark"><Icon name="error" size={12} /></span>
      <div class="error-copy">
        <p>{error}</p>
      </div>
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
    font-size: 0.85em;
  }

  input:disabled {
    opacity: 0.6;
  }

  button {
    font: inherit;
    font-size: 0.85em;
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
    opacity: 0.55;
    cursor: default;
  }

  .error-card {
    margin-top: 8px;
    font-size: 0.8em;
    word-break: break-word;
  }
</style>
