<script lang="ts">
  import IconButton from './IconButton.svelte';
  import { t } from '$lib/i18n/index.svelte';
  import { rise } from '$lib/motion';
  import { fly } from 'svelte/transition';

  interface Props {
    /** Title of the post the reader is showing, which this chat is not about. */
    title: string;
    disabled?: boolean;
    /** Hidden until the backend can hold more than one post per conversation. */
    onAdd?: () => void;
    onStartHere: () => void;
    onDismiss: () => void;
  }

  let { title, disabled = false, onAdd, onStartHere, onDismiss }: Props = $props();
</script>

<!-- Sits directly above the composer, where the user's attention already is,
     so noticing it costs no scanning (docs/AI_CONVERSATION_CONTEXT_PLAN.md §4.2). -->
<div class="suggestion" transition:fly={rise()}>
  <div class="suggestion-heading">
    <div class="suggestion-copy">
      <p class="explanation">{t('ai.suggest.explanation')}</p>
      <p class="headline" {title}>{title}</p>
    </div>
    <IconButton
      icon="close"
      size="sm"
      iconSize={12}
      label={t('ai.suggest.dismiss')}
      onclick={onDismiss}
    />
  </div>
  <div class="actions">
    {#if onAdd}<button
        type="button"
        class="primary ui-action ui-action--selected"
        {disabled}
        onclick={onAdd}>{t('ai.suggest.addToChat')}</button
      >{/if}
    <button type="button" class="ui-action" {disabled} onclick={onStartHere}
      >{t('ai.suggest.newChatHere')}</button
    >
  </div>
</div>

<style>
  .suggestion {
    padding: var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }
  .suggestion-heading {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }
  .suggestion-copy {
    flex: 1;
    min-width: 0;
  }
  .explanation {
    margin: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    line-height: 1.5;
  }
  .headline {
    margin: var(--space-1) 0 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
  .actions button:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
</style>
