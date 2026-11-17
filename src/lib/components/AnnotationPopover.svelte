<script lang="ts">
  import type { Annotation, AnnotationKind } from '$lib/types';

  interface Props {
    rect: DOMRect;
    kind: AnnotationKind;
    /** The existing annotation being viewed/edited, or null when creating a new memo. */
    annotation: Annotation | null;
    onSaveNote: (note: string) => void;
    onColorChange: (color: string) => void;
    onDelete: () => void;
    onClose: () => void;
  }

  let { rect, kind, annotation, onSaveNote, onColorChange, onDelete, onClose }: Props = $props();

  const COLORS = ['yellow', 'green', 'blue', 'pink', 'purple'] as const;
  const MAX_NOTE_LENGTH = 10_000;
  const POPOVER_WIDTH = 260;
  const MARGIN = 8;

  // The popover is always remounted fresh for a new annotation (the
  // {#if popover} block in FeedContent goes through null when closing), so
  // reading `annotation` only at mount time to seed the local draft is safe.
  // svelte-ignore state_referenced_locally
  let noteDraft = $state(annotation?.note ?? '');

  const top = $derived(
    Math.max(MARGIN, Math.min(rect.bottom + MARGIN, window.innerHeight - 160 - MARGIN)),
  );
  const left = $derived(
    Math.max(MARGIN, Math.min(rect.left, window.innerWidth - POPOVER_WIDTH - MARGIN)),
  );

  function save() {
    const trimmed = noteDraft.trim();
    if (!trimmed) return;
    onSaveNote(trimmed);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="backdrop" onclick={onClose}></div>
<div
  class="popover"
  style:top="{top}px"
  style:left="{left}px"
  style:width="{POPOVER_WIDTH}px"
  role="dialog"
  aria-label={kind === 'highlight' ? 'Highlight options' : 'Memo'}
>
  {#if kind === 'highlight'}
    <div class="swatches">
      {#each COLORS as color (color)}
        <button
          type="button"
          class="swatch"
          class:active={annotation?.color === color}
          style:background="var(--hl-{color})"
          aria-label="Change color to {color}"
          title={color}
          onclick={() => onColorChange(color)}
        ></button>
      {/each}
    </div>
    <div class="actions">
      <button type="button" class="danger" onclick={onDelete}>Remove highlight</button>
    </div>
  {:else}
    <textarea bind:value={noteDraft} maxlength={MAX_NOTE_LENGTH} placeholder="Add a note…" rows="4"
    ></textarea>
    <div class="note-footer">
      <span class="count">{noteDraft.length} / {MAX_NOTE_LENGTH}</span>
      <div class="actions">
        {#if annotation}
          <button type="button" class="danger" onclick={onDelete}>Delete</button>
        {/if}
        <button type="button" class="primary" disabled={!noteDraft.trim()} onclick={save}
          >Save</button
        >
      </div>
    </div>
  {/if}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 49;
  }

  .popover {
    position: fixed;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
    box-shadow: var(--shadow-popover);
  }

  .swatches {
    display: flex;
    gap: 8px;
  }

  .swatch {
    width: 26px;
    height: 26px;
    border: 2px solid transparent;
    border-radius: 50%;
    cursor: pointer;
  }

  .swatch.active {
    border-color: var(--color-accent);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  button.danger {
    border: 0;
    background: transparent;
    color: var(--color-error);
    font: inherit;
    font-size: 0.82em;
    cursor: pointer;
  }

  button.primary {
    padding: 6px 12px;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
    font: inherit;
    font-size: 0.82em;
    font-weight: 600;
    cursor: pointer;
  }

  button.primary:disabled {
    opacity: 0.5;
    cursor: default;
  }

  textarea {
    width: 100%;
    min-height: 80px;
    padding: 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
    color: var(--color-text);
    font: inherit;
    font-size: 0.85em;
    resize: vertical;
  }

  .note-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .count {
    color: var(--color-text-tertiary);
    font-size: 0.72em;
  }
</style>
