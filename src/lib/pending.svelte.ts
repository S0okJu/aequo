/**
 * Gates a loading indicator so fast work (most local SQLite reads) never
 * flashes one (docs/UNIFIED_UX_MOTION_PLAN.md §6.3). `delayed(busy)` reads
 * `true` only once `busy()` has stayed `true` continuously for `ms`
 * milliseconds, and snaps back to `false` the instant `busy()` does -
 * finishing quickly never leaves a stray spinner on screen a moment longer
 * than the work itself.
 *
 * Call from a component's (or another runed module's) top level - it uses
 * `$state`/`$effect`, which both need reactive/effect context.
 *
 * @example
 * const showLoading = delayed(() => loading && items.length === 0);
 * // in the template: {#if showLoading.value} ... {/if}
 */
export function delayed(busy: () => boolean, ms = 200): { readonly value: boolean } {
  let value = $state(false);

  $effect(() => {
    if (!busy()) {
      value = false;
      return;
    }
    const timer = setTimeout(() => {
      value = true;
    }, ms);
    return () => clearTimeout(timer);
  });

  return {
    get value() {
      return value;
    },
  };
}
