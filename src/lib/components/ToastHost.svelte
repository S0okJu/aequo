<script lang="ts">
  import { toasts, dismissToast } from '$lib/toast.svelte.ts';
  import Icon from '$lib/icons/Icon.svelte';
</script>

{#if toasts.length > 0}
  <div class="toast-host">
    {#each toasts as toast (toast.id)}
      <div
        class="toast"
        class:error={toast.type === 'error'}
        class:success={toast.type === 'success'}
        role={toast.type === 'error' ? 'alert' : 'status'}
        aria-live={toast.type === 'error' ? 'assertive' : 'polite'}
      >
        <span class="toast-icon">
          <Icon
            name={toast.type === 'error' ? 'error' : toast.type === 'success' ? 'success' : 'info'}
            size={14}
          />
        </span>
        <p>{toast.message}</p>
        {#if toast.action}
          {@const action = toast.action}
          <button
            type="button"
            class="toast-action"
            onclick={() => {
              dismissToast(toast.id);
              action.run();
            }}>{action.label}</button
          >
        {/if}
        <button
          type="button"
          class="toast-dismiss"
          aria-label="Dismiss notification"
          onclick={() => dismissToast(toast.id)}
        >
          <Icon name="close" size={12} />
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-host {
    position: fixed;
    bottom: 20px;
    left: 50%;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    transform: translateX(-50%);
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 220px;
    max-width: 420px;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
    box-shadow: var(--shadow-popover);
    pointer-events: auto;
    animation: toast-in 160ms ease;
  }

  .toast-icon {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    color: var(--color-accent);
  }

  .toast.success .toast-icon {
    color: var(--color-success);
  }

  .toast.error .toast-icon {
    color: var(--color-error);
  }

  .toast p {
    min-width: 0;
    flex: 1;
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-small);
    line-height: 1.4;
  }

  .toast-action {
    flex: 0 0 auto;
    padding: 2px var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-accent);
    font: inherit;
    font-size: var(--text-small);
    font-weight: var(--weight-bold);
    cursor: pointer;
  }

  .toast-action:hover {
    background: var(--color-hover-bg);
  }

  .toast-dismiss {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    padding: 2px;
    border: 0;
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
  }

  .toast-dismiss:hover {
    color: var(--color-text);
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .toast {
      animation: none;
    }
  }
</style>
