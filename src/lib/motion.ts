import { cubicOut } from 'svelte/easing';

/**
 * CSS transitions and animations are already switched off globally under
 * `prefers-reduced-motion` (src/routes/+layout.svelte). Svelte's JS-driven
 * transitions are not, so they scale their duration through this.
 */
export function prefersReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true
  );
}

/** `duration` in ms, or 0 when the user asked for less motion. */
export function motionDuration(duration: number): number {
  return prefersReducedMotion() ? 0 : duration;
}

/** Mirrors the CSS `--motion-*` tokens (docs/UNIFIED_UX_MOTION_PLAN.md §3.1)
 * for the JS-driven transitions (`fly`/`fade`) that can't read CSS vars. */
export const MOTION = { fast: 120, base: 180, layout: 220 } as const;

/**
 * Common entrance used across panes/popovers/toasts: fade in while rising
 * `--motion-rise` (6px) from below. One direction, one distance, everywhere -
 * don't pass custom `y`/`duration` to `fly` at a call site instead of this.
 */
export function rise(delay = 0): {
  y: number;
  duration: number;
  delay: number;
  easing: typeof cubicOut;
} {
  return {
    y: 6,
    duration: motionDuration(MOTION.base),
    delay: motionDuration(delay),
    easing: cubicOut,
  };
}

/** The list action bar enters from its own bottom edge. */
export function selectionBarMotion() {
  return { y: '100%', duration: motionDuration(MOTION.layout), easing: cubicOut };
}
