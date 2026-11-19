<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '$lib/icons/Icon.svelte';

  interface Props {
    /** Visible label; also names the control when `for` points at it. */
    label: string;
    for?: string;
    /** Without `for`, the label gets this id so a group can `aria-labelledby` it. */
    labelId?: string;
    hint?: string;
    /** Short save feedback shown next to the label ("Saved", an error). */
    status?: string | null;
    statusTone?: 'neutral' | 'error';
    /** Bump to show the same status text again after a later save. */
    statusKey?: number;
    children: Snippet;
  }

  let {
    label,
    for: htmlFor,
    labelId,
    hint,
    status = null,
    statusTone = 'neutral',
    statusKey = 0,
    children,
  }: Props = $props();

  const SUCCESS_VISIBLE_MS = 2000;

  // Success fades out on its own; an error stays until the next change.
  let statusVisible = $state(false);

  $effect(() => {
    void statusKey;
    statusVisible = status !== null;
    if (!status || statusTone === 'error') return;
    const timer = setTimeout(() => (statusVisible = false), SUCCESS_VISIBLE_MS);
    return () => clearTimeout(timer);
  });
</script>

<!-- One settings row: label and hint on the left, control on the right,
     wrapping under the label when the column is narrow
     (docs/UI_SIMPLIFICATION_PLAN.md §3.10). -->
<div class="settings-row">
  <div class="text">
    <div class="label-line">
      {#if htmlFor}
        <label class="label" for={htmlFor}>{label}</label>
      {:else}
        <span class="label" id={labelId}>{label}</span>
      {/if}
      <span class="status-slot" role="status">
        {#if status && statusVisible}
          {#key statusKey}
            <span class="status" class:error={statusTone === 'error'}>
              <Icon name={statusTone === 'error' ? 'error' : 'check'} size={11} />{status}
            </span>
          {/key}
        {/if}
      </span>
    </div>
    {#if hint}<p class="hint">{hint}</p>{/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .settings-row {
    display: grid;
    grid-template-columns: minmax(0, min(42%, 360px)) minmax(0, 1fr);
    align-items: center;
    gap: var(--space-2) var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .settings-row:last-child {
    border-bottom: 0;
  }

  .text {
    min-width: 0;
  }

  .label-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .label {
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
  }

  .hint {
    margin: var(--space-1) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.45;
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 8px 1px 6px;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--color-success) 14%, transparent);
    color: var(--color-success);
    font-size: var(--text-caption);
    font-weight: var(--weight-medium);
    /* Fade only, at --motion-fast: the status text replaces itself in the
       same spot, so this isn't a layout-speed move (docs/UNIFIED_UX_MOTION_PLAN.md §5). */
    animation: fade-in var(--motion-fast) var(--ease-enter) both;
  }

  .status.error {
    background: color-mix(in srgb, var(--color-error) 14%, transparent);
    color: var(--color-error);
  }

  .control {
    min-width: 0;
    width: min(100%, 360px);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .control :global(select) {
    max-width: 100%;
    min-width: 0;
    width: 100%;
  }

  .control :global(input[type='text']) {
    min-width: 180px;
    padding: 6px 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
  }
  .control :global(.segmented) {
    width: 100%;
    min-width: 0;
  }
  @container settings-fields (max-width: 540px) {
    .settings-row {
      grid-template-columns: minmax(0, 1fr);
    }
    .settings-row:has(:global([role='switch'])) {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .settings-row:has(:global([role='switch'])) .control {
      width: auto;
    }
  }
</style>
