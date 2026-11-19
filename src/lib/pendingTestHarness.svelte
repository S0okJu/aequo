<script lang="ts">
  import { delayed } from './pending.svelte';

  /**
   * Test-only host for `delayed()` - `$effect`/`$state` only run inside a
   * mounted component, so `pending.test.ts` drives this through
   * @testing-library/svelte's `render`/`rerender` instead of calling the
   * rune-based function directly from plain test code.
   */
  interface Props {
    busy: boolean;
    ms?: number;
  }

  let { busy, ms = 200 }: Props = $props();
  // `ms` is only ever read once by `delayed()` at setup, matching its own
  // signature - not meant to track later prop changes.
  // svelte-ignore state_referenced_locally
  const state = delayed(() => busy, ms);
</script>

<span data-testid="value">{state.value}</span>
