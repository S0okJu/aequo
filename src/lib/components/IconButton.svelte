<script lang="ts">
  import Icon from '$lib/icons/Icon.svelte';
  import type { IconName } from '$lib/icons';

  /**
   * One icon-only button shape for every pane (docs/UNIFIED_UX_MOTION_PLAN.md
   * §4): same three sizes, same hover/active/disabled/spinning treatment,
   * same accessible-name contract (`label` is both the a11y name and the
   * tooltip unless `titleOverride` says otherwise - e.g. a tooltip that
   * reports cached/live state while the a11y name stays constant).
   */
  interface Props {
    icon: IconName;
    label: string;
    size?: 'sm' | 'md' | 'lg';
    iconSize?: number;
    /** Omit entirely for a plain action button (no aria-pressed at all).
     * Pass `true`/`false` for a toggle - both render aria-pressed, so "off"
     * still announces as a toggle rather than looking like a disabled one. */
    active?: boolean;
    hasDot?: boolean;
    spinning?: boolean;
    disabled?: boolean;
    href?: string;
    type?: 'button' | 'submit';
    titleOverride?: string;
    /** Extra class(es) for layout only (e.g. absolute-positioning a copy
     * button inside a message bubble) - never for re-skinning the button
     * itself, which stays centrally defined here. */
    class?: string;
    /** For a button that opens a menu/dialog/popover in place. */
    ariaHaspopup?: 'menu' | 'dialog' | 'listbox' | 'true';
    ariaExpanded?: boolean;
    onclick?: (event: MouseEvent) => void;
  }

  let {
    icon,
    label,
    size = 'md',
    iconSize,
    active,
    hasDot = false,
    spinning = false,
    disabled = false,
    href,
    type = 'button',
    titleOverride,
    class: extraClass = '',
    ariaHaspopup,
    ariaExpanded,
    onclick,
  }: Props = $props();

  const DEFAULT_ICON_SIZE = { sm: 13, md: 15, lg: 16 } as const;
  const resolvedIconSize = $derived(iconSize ?? DEFAULT_ICON_SIZE[size]);
  const title = $derived(titleOverride ?? label);
</script>

{#if href}
  <!-- eslint-disable svelte/no-navigation-without-resolve -->
  <a
    class="icon-button {size} {extraClass}"
    class:active
    class:has-dot={hasDot}
    {href}
    target="_blank"
    rel="noopener noreferrer"
    aria-label={label}
    {title}><Icon name={icon} size={resolvedIconSize} /></a
  >
  <!-- eslint-enable svelte/no-navigation-without-resolve -->
{:else}
  <button
    class="icon-button {size} {extraClass}"
    class:active
    class:has-dot={hasDot}
    class:spinning
    {type}
    {disabled}
    aria-label={label}
    aria-pressed={active === undefined ? undefined : active}
    aria-haspopup={ariaHaspopup}
    aria-expanded={ariaExpanded}
    {title}
    {onclick}><Icon name={icon} size={resolvedIconSize} /></button
  >
{/if}

<style>
  .icon-button {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    border: 0;
    border-radius: var(--control-radius);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: background-color var(--transition);
  }

  .icon-button.sm {
    width: var(--control-size-xs);
    height: var(--control-size-xs);
  }

  .icon-button.md {
    width: var(--control-size);
    height: var(--control-size);
  }

  .icon-button.lg {
    width: var(--control-size-lg);
    height: var(--control-size-lg);
  }

  .icon-button:hover:not(:disabled) {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  .icon-button.active {
    color: var(--color-text);
    background: var(--color-selection-bg);
  }

  .icon-button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* A dot, not a count: it says "there is something here" without asking to
     be read (docs/UNIFIED_UX_MOTION_PLAN.md §4, replaces each screen's own
     .has-history-style decoration). */
  .icon-button.has-dot {
    position: relative;
  }

  .icon-button.has-dot::after {
    position: absolute;
    top: 5px;
    right: 5px;
    width: 5px;
    height: 5px;
    border-radius: var(--radius-pill);
    background: var(--color-accent);
    content: '';
  }

  /* Shared `spin` keyframe is declared once, globally, in +layout.svelte -
     see scripts/check-motion.sh rule 3 (one spinner animation in the app). */
  .icon-button.spinning :global(svg) {
    animation: spin var(--motion-loop) linear infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .icon-button.spinning :global(svg) {
      animation: none;
    }
  }
</style>
