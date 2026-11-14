<script lang="ts">
  let { children } = $props();
</script>

{@render children()}

<style>
  :global(:root) {
    /* Neutral grayscale base + one accent (design principles, M7).
       Tokenized so dark mode (13장 roadmap, not v1) can swap values
       later without touching layout. */
    --color-bg: #ffffff;
    --color-bg-secondary: #f6f6f7;
    --color-border: #e2e2e4;
    --color-text: #1a1a1a;
    --color-text-secondary: #6e6e73;
    --color-text-tertiary: #9a9a9e;
    --color-accent: #0a7cff;
    --color-selection-bg: #e5f1ff;
    --color-hover-bg: #f0f0f1;
  }

  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --color-bg: #1e1e1e;
      --color-bg-secondary: #252526;
      --color-border: #3a3a3c;
      --color-text: #f0f0f0;
      --color-text-secondary: #a0a0a5;
      --color-text-tertiary: #7a7a7e;
      --color-accent: #4aa3ff;
      --color-selection-bg: #1c3a5e;
      --color-hover-bg: #2a2a2c;
    }
  }

  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Text', sans-serif;
    background: var(--color-bg);
    color: var(--color-text);
  }

  :global(*) {
    box-sizing: border-box;
  }
</style>
