<script lang="ts">
  import { addFeed } from '$lib/api';

  interface Props {
    onAdded: () => void;
  }

  let { onAdded }: Props = $props();

  let url = $state('');
  let error = $state<string | null>(null);
  let submitting = $state(false);

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    const trimmed = url.trim();
    if (!trimmed || submitting) return;

    submitting = true;
    error = null;
    try {
      await addFeed(trimmed);
      url = '';
      onAdded();
    } catch (err) {
      error = String(err);
    } finally {
      submitting = false;
    }
  }
</script>

<form class="add-feed" onsubmit={handleSubmit}>
  <input
    type="url"
    placeholder="Add feed URL…"
    bind:value={url}
    disabled={submitting}
    aria-label="Feed URL"
  />
  {#if error}
    <p class="error">{error}</p>
  {/if}
</form>

<style>
  .add-feed {
    padding: 8px;
    border-top: 1px solid var(--color-border);
  }

  input {
    width: 100%;
    padding: 6px 8px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background: var(--color-bg);
    color: var(--color-text);
    font-size: 0.85em;
  }

  input:disabled {
    opacity: 0.6;
  }

  .error {
    margin: 6px 0 0;
    color: var(--color-error);
    font-size: 0.8em;
    word-break: break-word;
  }
</style>
