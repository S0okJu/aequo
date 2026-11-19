<script lang="ts">
  import type { AiProvider } from '$lib/types';

  let {
    value,
    label,
    hint,
    disabled = false,
    compact = false,
    onChange,
    resetKey,
  }: {
    value: AiProvider;
    label: string;
    hint?: string;
    disabled?: boolean;
    compact?: boolean;
    onChange: (provider: AiProvider) => void;
    /** Bump this to force the <select> to snap back to `value` even when
     * `value` itself hasn't changed - e.g. the caller declined to act on a
     * selection it asked the user to confirm first. */
    resetKey?: number | string;
  } = $props();

  const id = $props.id();
</script>

<div class="picker" class:compact>
  <label for={id} class:visually-hidden={compact}>{label}</label>
  {#key resetKey}
    <select
      {id}
      {value}
      {disabled}
      aria-describedby={hint ? `${id}-hint` : undefined}
      title={hint ?? label}
      onchange={(event) => onChange(event.currentTarget.value as AiProvider)}
    >
      <option value="codex">Codex</option>
      <option value="claude">Claude Code</option>
    </select>
  {/key}
  {#if hint}<p id="{id}-hint" class="hint" class:visually-hidden={compact}>{hint}</p>{/if}
</div>

<style>
  .picker {
    min-width: 0;
  }
  label {
    display: block;
    margin-bottom: var(--space-2);
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
  }
  select {
    min-height: var(--control-size);
    max-width: 100%;
    padding: var(--space-1) var(--space-5) var(--space-1) var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--control-radius);
    background-color: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    cursor: pointer;
  }
  select:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  select:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .compact select {
    border-color: transparent;
    background-color: transparent;
    font-size: var(--text-small);
    color: var(--color-text-secondary);
  }
  .compact select:hover:not(:disabled) {
    background-color: var(--color-hover-bg);
    color: var(--color-text);
  }
  .hint {
    margin: var(--space-2) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
  }
</style>
