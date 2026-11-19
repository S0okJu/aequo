<script lang="ts">
  /**
   * The one "working on it, inline" indicator (docs/UNIFIED_UX_MOTION_PLAN.md
   * §4, §5.3 - a separate shape from the circular Spinner, but still a
   * single-sourced component rather than copy-pasted CSS). The parent owns
   * the surrounding live region (e.g. the streaming message bubble's
   * `aria-live="polite" aria-busy="true"`) - this is presentation only.
   */
  interface Props {
    label: string;
  }

  let { label }: Props = $props();
</script>

<div class="thinking">
  <span></span><span></span><span></span>
  {label}
</div>

<style>
  .thinking {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .thinking span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    animation: pulse var(--motion-loop-slow) infinite ease-in-out;
  }

  .thinking span:nth-child(2) {
    animation-delay: 0.15s;
  }

  .thinking span:nth-child(3) {
    margin-right: var(--space-1);
    animation-delay: 0.3s;
  }

  @keyframes pulse {
    0%,
    80%,
    100% {
      opacity: 0.25;
    }
    40% {
      opacity: 1;
    }
  }
</style>
