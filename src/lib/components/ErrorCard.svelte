<script lang="ts">
  import Icon from '$lib/icons/Icon.svelte';
  import { t } from '$lib/i18n/index.svelte';

  /**
   * The one "something you did failed, and you can do it again from here"
   * card (docs/UNIFIED_UX_MOTION_PLAN.md §6.2 "T2 영역 카드"). Wraps the
   * existing global `.error-card` classes (src/routes/+layout.svelte,
   * originally AiPanel's error card) rather than introducing new CSS -
   * AiPanel's own error card is the first thing migrated onto this
   * component, so its markup is the reference implementation.
   */
  interface Props {
    title: string;
    body: string;
    onRetry?: () => void | Promise<void>;
    retryLabel?: string;
    retrying?: boolean;
    retryingLabel?: string;
    /** Omit entirely for a non-dismissible card. */
    onDismiss?: () => void;
    dismissLabel?: string;
  }

  let {
    title,
    body,
    onRetry,
    retryLabel,
    retrying = false,
    retryingLabel,
    onDismiss,
    dismissLabel,
  }: Props = $props();
</script>

<div class="error-card" role="alert">
  <span class="error-mark"><Icon name="error" size={12} /></span>
  <div class="error-copy">
    <strong>{title}</strong>
    <p>{body}</p>
  </div>
  {#if onRetry}
    <button type="button" disabled={retrying} onclick={onRetry}>
      {retrying ? (retryingLabel ?? t('common.retrying')) : (retryLabel ?? t('common.retry'))}
    </button>
  {/if}
  {#if onDismiss}
    <button
      class="dismiss-error"
      type="button"
      aria-label={dismissLabel ?? t('common.dismissError')}
      onclick={onDismiss}><Icon name="close" size={13} /></button
    >
  {/if}
</div>
