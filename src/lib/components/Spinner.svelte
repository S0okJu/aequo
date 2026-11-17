<script lang="ts">
  interface Props {
    size?: number;
  }

  let { size = 17 }: Props = $props();
</script>

<span
  role="status"
  aria-label="Loading"
  class="spinner"
  style:width="{size}px"
  style:height="{size}px"
></span>

<style>
  .spinner {
    display: inline-block;
    flex: 0 0 auto;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 800ms linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation: none;
    }
  }
</style>
