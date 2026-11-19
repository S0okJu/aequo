<script lang="ts" generics="T extends string | number | boolean">
  import Icon from '$lib/icons/Icon.svelte';
  import type { IconName } from '$lib/icons';

  interface Option {
    value: T;
    label: string;
    icon?: IconName;
    /** A small count after the label (e.g. unread posts); hidden when 0. */
    badge?: number;
  }

  interface Props {
    options: Option[];
    value: T;
    onChange: (value: T) => void;
    /** Names the group; pass one of these two. */
    ariaLabel?: string;
    labelledby?: string;
    /** `sm` sits in pane headers, `md` in settings rows. */
    size?: 'sm' | 'md';
    /** Fill the row, splitting it evenly between options. */
    block?: boolean;
    disabled?: boolean;
  }

  let {
    options,
    value,
    onChange,
    ariaLabel,
    labelledby,
    size = 'md',
    block = false,
    disabled = false,
  }: Props = $props();

  let group = $state<HTMLDivElement | undefined>(undefined);
  let buttons = $state<HTMLButtonElement[]>([]);
  let thumb = $state<{ left: number; width: number } | null>(null);
  // The thumb is placed without a transition first, then slides from there.
  let ready = $state(false);

  const selectedIndex = $derived(options.findIndex((option) => option.value === value));

  function measure() {
    const button = buttons[selectedIndex];
    thumb = button ? { left: button.offsetLeft, width: button.offsetWidth } : null;
  }

  $effect(() => {
    void selectedIndex;
    void options.length;
    measure();
  });

  // Labels change width with the locale and the window; keep the thumb on its button.
  $effect(() => {
    if (!group) return;
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(group);
    for (const button of buttons) if (button) observer?.observe(button);
    const frame = requestAnimationFrame(() => (ready = true));
    return () => {
      observer?.disconnect();
      cancelAnimationFrame(frame);
    };
  });

  function select(index: number) {
    if (disabled) return;
    const option = options[index];
    if (!option) return;
    if (option.value !== value) onChange(option.value);
    buttons[index]?.focus();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (disabled) return;
    const last = options.length - 1;
    const current = Math.max(0, selectedIndex);
    const next =
      event.key === 'ArrowRight' || event.key === 'ArrowDown'
        ? current === last
          ? 0
          : current + 1
        : event.key === 'ArrowLeft' || event.key === 'ArrowUp'
          ? current === 0
            ? last
            : current - 1
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? last
              : null;
    if (next === null) return;
    event.preventDefault();
    select(next);
  }
</script>

<!-- One look for every single choice in the app (docs/SIDEBAR_SETTINGS_UX_GOAL.md §4):
     a radio group whose selection is a thumb that slides between options. -->
<div
  bind:this={group}
  class="segmented {size}"
  class:block
  class:ready
  class:disabled
  role="radiogroup"
  aria-label={ariaLabel}
  aria-labelledby={labelledby}
  aria-disabled={disabled || undefined}
  tabindex="-1"
  onkeydown={handleKeydown}
>
  {#if thumb}
    <span
      class="thumb"
      aria-hidden="true"
      style:width="{thumb.width}px"
      style:transform="translateX({thumb.left}px)"
    ></span>
  {/if}
  {#each options as option, index (option.value)}
    <button
      bind:this={buttons[index]}
      type="button"
      {disabled}
      role="radio"
      aria-checked={option.value === value}
      tabindex={index === Math.max(0, selectedIndex) ? 0 : -1}
      onclick={() => select(index)}
    >
      {#if option.icon}<Icon name={option.icon} size={size === 'sm' ? 12 : 14} />{/if}
      {option.label}
      {#if option.badge}<span class="badge" aria-hidden="true">{option.badge}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .segmented {
    position: relative;
    display: inline-flex;
    flex-shrink: 0;
    flex-wrap: nowrap;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--color-hover-bg);
  }

  .segmented.block {
    display: flex;
    min-width: 0;
    flex: 1;
  }

  .segmented.disabled {
    opacity: 0.55;
  }

  button:disabled {
    cursor: default;
  }

  .block button {
    min-width: 0;
    flex: 1 1 0;
  }

  .segmented:focus {
    outline: none;
  }

  .thumb {
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 0;
    border-radius: var(--radius-xs);
    background: var(--color-segment-thumb);
    box-shadow:
      0 1px 2px rgba(0, 0, 0, 0.1),
      0 0 0 0.5px rgba(0, 0, 0, 0.04);
  }

  .ready .thumb {
    transition:
      transform var(--transition-layout),
      width var(--transition-layout);
  }

  button {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }

  .sm button {
    min-height: calc(var(--control-size) - 4px);
    padding: 2px var(--space-2);
    font-size: var(--text-small);
  }

  .md button {
    min-height: calc(var(--control-size) - 4px);
    padding: 4px var(--space-3);
    font-size: var(--text-body);
  }

  button:hover {
    color: var(--color-text);
  }

  button[aria-checked='true'] {
    color: var(--color-text);
    font-weight: var(--weight-medium);
  }

  .badge {
    min-width: 16px;
    padding: 0 5px;
    border-radius: var(--radius-pill);
    background: var(--color-border);
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
    font-weight: var(--weight-medium);
    font-variant-numeric: tabular-nums;
    line-height: 16px;
    text-align: center;
    transition:
      background-color var(--transition),
      color var(--transition);
  }

  button[aria-checked='true'] .badge {
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
  }

  button:focus-visible {
    outline-offset: 0;
  }
</style>
