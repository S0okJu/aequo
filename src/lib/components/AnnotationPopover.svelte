<script lang="ts">
  import { updateDraft } from '$lib/updates.svelte';
  import type { Annotation, AnnotationKind } from '$lib/types';
  import { t } from '$lib/i18n/index.svelte';

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
  let popoverEl = $state<HTMLDivElement>();
  let textareaEl = $state<HTMLTextAreaElement>();
  // Measured after mount - a guessed height meant the popover could open
  // below the viewport for a tall note (docs/UNIFIED_UX_MOTION_PLAN.md
  // §7.5 A5). 160 is only the pre-measurement fallback for this same frame.
  let measuredHeight = $state(160);

  $effect(() => {
    if (popoverEl) measuredHeight = popoverEl.getBoundingClientRect().height;
  });

  $effect(() => {
    if (kind === 'memo') textareaEl?.focus();
    else popoverEl?.querySelector<HTMLButtonElement>('.swatch')?.focus();
  });

  const top = $derived(
    Math.max(MARGIN, Math.min(rect.bottom + MARGIN, window.innerHeight - measuredHeight - MARGIN)),
  );
  const left = $derived(
    Math.max(MARGIN, Math.min(rect.left, window.innerWidth - POPOVER_WIDTH - MARGIN)),
  );

  const dirty = $derived(kind === 'memo' && noteDraft.trim() !== (annotation?.note ?? ''));
  const updateDraftToken = Symbol('annotation-draft');
  $effect(() => {
    updateDraft(updateDraftToken, dirty);
  });
  $effect(() => () => updateDraft(updateDraftToken, false));

  function save() {
    const trimmed = noteDraft.trim();
    if (!trimmed) return;
    onSaveNote(trimmed);
  }

  // A backdrop click is the one way to leave that isn't a deliberate
  // "discard this" choice (unlike Escape or the dialog's own buttons), so
  // it's the one that's ignored instead of silently dropping an unsaved
  // note (docs/UNIFIED_UX_MOTION_PLAN.md §7.5 A4).
  function handleBackdropClick() {
    if (!dirty) onClose();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== 'Tab' || !popoverEl) return;
    const focusable = [
      ...popoverEl.querySelectorAll<HTMLElement>(
        'button, [href], textarea, [tabindex]:not([tabindex="-1"])',
      ),
    ];
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="backdrop" onclick={handleBackdropClick}></div>
<div
  class="popover"
  bind:this={popoverEl}
  tabindex="-1"
  style:top="{top}px"
  style:left="{left}px"
  style:width="{POPOVER_WIDTH}px"
  role="dialog"
  aria-label={kind === 'highlight' ? t('annotation.highlightOptions') : t('annotation.memo')}
  onkeydown={handleKeydown}
>
  {#if kind === 'highlight'}
    <div class="swatches">
      {#each COLORS as color (color)}
        <button
          type="button"
          class="swatch"
          class:active={annotation?.color === color}
          style:background="var(--hl-{color})"
          aria-label={t('annotation.changeColor', { color: t(`color.${color}`) })}
          title={color}
          onclick={() => onColorChange(color)}
        ></button>
      {/each}
    </div>
    <div class="actions">
      <button type="button" class="ui-action ui-action--danger" onclick={onDelete}
        >{t('annotation.removeHighlight')}</button
      >
    </div>
  {:else}
    <textarea
      bind:value={noteDraft}
      bind:this={textareaEl}
      maxlength={MAX_NOTE_LENGTH}
      placeholder={t('annotation.notePlaceholder')}
      rows="4"></textarea>
    <div class="note-footer">
      <span class="count">{noteDraft.length} / {MAX_NOTE_LENGTH}</span>
      <div class="actions">
        {#if annotation}
          <button type="button" class="ui-action ui-action--danger" onclick={onDelete}
            >{t('common.delete')}</button
          >
        {/if}
        <button
          type="button"
          class="ui-action ui-action--primary"
          disabled={!noteDraft.trim()}
          onclick={save}>{t('common.save')}</button
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
    gap: var(--space-2);
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
    box-shadow: var(--shadow-popover);
  }

  .swatches {
    display: flex;
    gap: var(--space-2);
  }

  /* Same rest/active swatch treatment as SelectionToolbar.svelte - an
     opaque preview color needs a real border to read in dark mode; a
     transparent one at rest was nearly invisible there
     (docs/UNIFIED_UX_MOTION_PLAN.md §7.5 D3). border-box keeps the 26px
     box size constant when active's border goes from 1px to 2px. */
  .swatch {
    width: 26px;
    height: 26px;
    border: 1px solid var(--color-border);
    border-radius: 50%;
    cursor: pointer;
    transition: border-color var(--transition);
  }

  .swatch:hover {
    border-color: var(--color-accent);
  }

  .swatch.active {
    border: 2px solid var(--color-accent);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  textarea {
    width: 100%;
    min-height: 80px;
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    resize: vertical;
  }

  .note-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .count {
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }
</style>
