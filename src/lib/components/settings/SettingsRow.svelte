<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    /** Visible label; also names the control when `for` points at it. */
    label: string;
    for?: string;
    hint?: string;
    /** Short save feedback shown next to the label ("Saved", an error). */
    status?: string | null;
    statusTone?: 'neutral' | 'error';
    children: Snippet;
  }

  let {
    label,
    for: htmlFor,
    hint,
    status = null,
    statusTone = 'neutral',
    children,
  }: Props = $props();
</script>

<!-- One settings row: label and hint on the left, control on the right,
     wrapping under the label when the column is narrow
     (docs/UI_SIMPLIFICATION_PLAN.md §3.10). -->
<div class="settings-row">
  <div class="text">
    <label class="label" for={htmlFor}>{label}</label>
    {#if hint}<p class="hint">{hint}</p>{/if}
    {#if status}
      <p class="status" class:error={statusTone === 'error'} role="status">{status}</p>
    {/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .settings-row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2) var(--space-5);
    padding: var(--space-4) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .text {
    min-width: 200px;
    flex: 1 1 260px;
  }

  .label {
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
  }

  .hint,
  .status {
    margin: var(--space-1) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.45;
  }

  .status {
    color: var(--color-success);
  }

  .status.error {
    color: var(--color-error);
  }

  .control {
    display: flex;
    flex: 0 1 auto;
    align-items: center;
    gap: var(--space-2);
  }

  .control :global(select),
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
</style>
