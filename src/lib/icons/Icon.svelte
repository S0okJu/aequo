<script lang="ts">
  import { ICONS, type IconName } from './index';

  interface Props {
    name: IconName;
    size?: number;
    class?: string;
  }

  let { name, size = 16, class: className = '' }: Props = $props();
</script>

<svg
  class={className}
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  <!-- Fixed, hand-written path data from ./index.ts - never user input. -->
  <!-- eslint-disable-next-line svelte/no-at-html-tags -->
  {@html ICONS[name]}
</svg>
