<script lang="ts">
  import AddFeedForm from './AddFeedForm.svelte';
  import { t } from '$lib/i18n/index.svelte';
  import type { FeedFolder } from '$lib/types';

  interface Props {
    open: boolean;
    folders: FeedFolder[];
    /** Preselected folder, e.g. the one the sidebar currently shows. */
    defaultFolderId?: number | null;
    onAdded: (feedId: number) => void;
    onClose: () => void;
  }

  let { open, folders, defaultFolderId = null, onAdded, onClose }: Props = $props();

  let dialog = $state<HTMLDialogElement | undefined>(undefined);

  // A native modal <dialog> gives focus trapping, Escape-to-close and focus
  // return to the opener for free (docs/UI_SIMPLIFICATION_PLAN.md §3.11).
  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });
</script>

<dialog
  bind:this={dialog}
  class="add-feed-dialog"
  aria-labelledby="add-feed-title"
  onclose={onClose}
  onclick={(event) => {
    // A click on the dialog box itself (not its content) is the backdrop.
    if (event.target === dialog) onClose();
  }}
>
  {#if open}
    <div class="panel">
      <h2 id="add-feed-title">{t('addFeed.title')}</h2>
      <p class="hint">{t('addFeed.hint')}</p>
      <AddFeedForm {folders} {defaultFolderId} {onAdded} onCancel={onClose} />
    </div>
  {/if}
</dialog>

<style>
  .add-feed-dialog {
    width: min(440px, calc(100vw - 32px));
    padding: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-xl);
    background: var(--color-bg);
    color: var(--color-text);
    box-shadow: var(--shadow-popover);
  }

  .add-feed-dialog::backdrop {
    background: rgba(0, 0, 0, 0.25);
  }

  .panel {
    padding: var(--space-5);
  }

  h2 {
    margin: 0 0 var(--space-1);
    font-size: var(--text-heading);
    font-weight: var(--weight-bold);
  }

  .hint {
    margin: 0 0 var(--space-4);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.45;
  }
</style>
