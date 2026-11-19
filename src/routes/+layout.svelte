<script lang="ts">
  import '$lib/styles/controls.css';
  import { onMount } from 'svelte';
  import { applyThemePreference, getThemePreference } from '$lib/theme';
  import ToastHost from '$lib/components/ToastHost.svelte';
  import { applyDocumentLanguage } from '$lib/i18n/index.svelte';
  import { startFeedIconSync } from '$lib/feedIcons.svelte';
  import { startResourceSync } from '$lib/resources.svelte';
  import { startUpdateSync } from '$lib/updates.svelte';

  let { children } = $props();

  onMount(() => {
    applyThemePreference(getThemePreference());
    applyDocumentLanguage();
    const stopIcons = startFeedIconSync();
    const stopResources = startResourceSync();
    const stopUpdates = startUpdateSync();
    return () => {
      stopUpdates();
      stopResources();
      stopIcons();
    };
  });
</script>

{@render children()}
<ToastHost />

<style>
  :global(:root) {
    color-scheme: light;
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
    --color-accent-solid: #0868d6;
    --color-on-accent: #ffffff;
    --color-on-error: #ffffff;
    --color-success: #268257;
    --color-selection-bg: #ededee;
    --color-hover-bg: #f0f0f1;
    --color-segment-thumb: #ffffff;
    --color-error: #d1453b;
    --code-keyword: #7c3aed;
    --code-string: #16803c;
    --code-number: #b45309;
    --code-comment: #85858b;
    --code-function: #1265b3;

    /* Highlight palette for reader annotations (docs/READER_ANNOTATIONS_PLAN.md
       §4.1) - fixed 5 colors, no custom picker. Semi-transparent so they
       layer over body text without hiding it. */
    --hl-yellow: rgba(255, 212, 0, 0.35);
    --hl-green: rgba(52, 199, 89, 0.3);
    --hl-blue: rgba(10, 124, 255, 0.25);
    --hl-pink: rgba(255, 90, 165, 0.3);
    --hl-purple: rgba(150, 90, 255, 0.3);

    /* Loading placeholder behind a body image with no width/height
       (docs/UNIFIED_UX_MOTION_PLAN.md §7.5 D5) - most web images are
       light-background screenshots/diagrams, so a light neutral plate
       previews closer to the real thing than the app's own surface color.
       Same value in light mode; dark mode overrides it below. */
    --color-image-plate: var(--color-bg-secondary);

    /* Shared shape/motion scale so every component picks from the same
       steps instead of inventing its own px value. */
    --radius-xs: 4px;
    --radius-sm: 6px;
    --radius-md: 8px;
    --radius-lg: 10px;
    --radius-xl: 12px;
    --radius-pill: 999px;
    --control-size: 32px;
    --control-size-xs: 24px;
    --control-size-lg: 36px;
    --control-radius: var(--radius-sm);
    --surface-radius: var(--radius-md);

    /* Motion scale (docs/UNIFIED_UX_MOTION_PLAN.md §3.1) - three durations,
       two easings, one rise distance. `--transition`/`--transition-layout`
       are kept as the names every component already uses; they now just
       point at these so there's one place to change the actual numbers.
       `pnpm check` enforces var(--motion-*) usage via scripts/check-motion.sh. */
    --motion-context-highlight: 600ms;
    --motion-fast: 120ms;
    --motion-base: 180ms;
    --motion-layout: 220ms;
    --motion-emphasis: 1200ms;
    --motion-loop: 800ms;
    --motion-loop-slow: 1200ms;
    --ease-move: cubic-bezier(0.4, 0, 0.2, 1);
    --ease-enter: cubic-bezier(0.2, 0, 0, 1);
    --motion-rise: 6px;
    --transition: var(--motion-fast) var(--ease-move);
    --transition-layout: var(--motion-layout) var(--ease-move);
    --shadow-popover: 0 4px 16px rgba(0, 0, 0, 0.12);

    /* Spacing scale (UX polish pass) - one shared rhythm for new/touched
       code instead of another one-off px value per component. */
    --space-1: 4px;
    --space-2: 8px;
    --space-3: 12px;
    --space-4: 16px;
    --space-5: 24px;
    --space-6: 32px;

    /* Role-based type scale (docs/UI_SIMPLIFICATION_PLAN.md §3.13). Every
       component font-size must be one of these - px, not em, so a size
       never compounds through nesting. Nothing renders below --text-caption.
       `pnpm check` enforces this via scripts/check-type-scale.sh. */
    --text-caption: 11px;
    --text-small: 12px;
    --text-body: 13px;
    --text-title: 14px;
    --text-heading: 17px;
    --text-display: 22px;
    --text-reading: 16px;
    /* Body-content heading levels (docs/UNIFIED_UX_MOTION_PLAN.md §7.2 R3) -
       separate from the UI's own --text-title/--text-heading/--text-display
       so a feed's own <h1>-<h3> can never render larger than the article
       title (--text-display) above it. Weight carries the h4 step instead
       of a fifth size. */
    --text-reading-h1: 20px;
    --text-reading-h2: 18px;
    --text-reading-h3: 16px;
    --weight-regular: 400;
    --weight-medium: 550;
    --weight-bold: 650;

    /* Readability caps for the desktop-only responsive pass: prose text
       stays at a comfortable line length, and card lists stop stretching
       edge-to-edge on an ultrawide window. */
    --content-max-width: 72ch;
    --pane-content-max-width: 820px;
  }

  :global(:root[data-theme='dark']) {
    --shadow-popover: 0 4px 16px rgba(0, 0, 0, 0.24);
    color-scheme: dark;
    --color-bg: #2a2d34;
    --color-bg-secondary: #212328;
    --color-border: #3c4048;
    --color-text: #d6d9de;
    --color-text-secondary: #a3a8b1;
    --color-text-tertiary: #83878f;
    --color-accent: #7fb0f0;
    --color-accent-solid: #7fb0f0;
    --color-on-accent: #182535;
    --color-on-error: #321b19;
    --color-success: #85cf97;
    --color-selection-bg: #3c4048;
    --color-hover-bg: #2f323a;
    --color-segment-thumb: #454a55;
    --color-error: #e8827a;
    --code-keyword: #b89cf0;
    --code-string: #85cf97;
    --code-number: #e8b06d;
    --code-comment: #8f8f96;
    --code-function: #80b5e8;
    /* All five colors had converged to the same 0.22 alpha, which both cut
       their contrast margin close and made them hard to tell apart from
       each other and from --color-selection-bg/--color-accent (both blue-
       leaning) - re-tuned per color, verified by script against
       --color-text (docs/UNIFIED_UX_MOTION_PLAN.md §7.4 D2/D4). Hue/alpha
       only; the palette is still 5 colors with the same names, so stored
       `color` values on existing annotations don't change meaning. */
    --hl-yellow: rgba(255, 212, 0, 0.3);
    --hl-green: rgba(52, 199, 89, 0.28);
    --hl-blue: rgba(70, 165, 255, 0.26);
    --hl-pink: rgba(255, 60, 140, 0.3);
    --hl-purple: rgba(175, 100, 255, 0.3);
    --color-image-plate: #e3e3e5;
  }

  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light']):not([data-theme='dark'])) {
      color-scheme: dark;
      --color-bg: #2a2d34;
      --color-bg-secondary: #212328;
      --color-border: #3c4048;
      --color-text: #d6d9de;
      --color-text-secondary: #a3a8b1;
      --color-text-tertiary: #83878f;
      --color-accent: #7fb0f0;
      --color-accent-solid: #7fb0f0;
      --color-on-accent: #182535;
      --color-on-error: #321b19;
      --color-success: #85cf97;
      --color-selection-bg: #3c4048;
      --color-hover-bg: #2f323a;
      --color-segment-thumb: #454a55;
      --color-segment-thumb: #454a55;
      --color-error: #e8827a;
      --code-keyword: #b89cf0;
      --code-string: #85cf97;
      --code-number: #e8b06d;
      --code-comment: #8f8f96;
      --code-function: #80b5e8;

      /* Kept identical to the explicit :root[data-theme='dark'] block above -
         see its comment (docs/UNIFIED_UX_MOTION_PLAN.md §7.4 D2/D4). */
      --hl-yellow: rgba(255, 212, 0, 0.3);
      --hl-green: rgba(52, 199, 89, 0.28);
      --hl-blue: rgba(70, 165, 255, 0.26);
      --hl-pink: rgba(255, 60, 140, 0.3);
      --hl-purple: rgba(175, 100, 255, 0.3);
      --color-image-plate: #e3e3e5;
    }
  }

  :global(body) {
    margin: 0;
    min-height: 100%;
    font-family:
      -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Apple SD Gothic Neo', sans-serif;
    font-size: var(--text-body);
    line-height: 1.5;
    background: var(--color-bg-secondary);
    color: var(--color-text);
  }

  :global(html) {
    min-height: 100%;
    background: var(--color-bg-secondary);
  }

  :global(*) {
    box-sizing: border-box;
  }

  /* One hover/focus language for every button in the app, so panes built
     independently (feed list, posts, reader, AI panel, settings) still
     feel like the same product. Components that need a different focus
     treatment (e.g. the drag handles) set their own and win on specificity. */
  :global(button) {
    transition:
      background-color var(--transition),
      border-color var(--transition),
      color var(--transition),
      opacity var(--transition);
  }

  :global(button:focus-visible),
  :global(a:focus-visible),
  :global(input:focus-visible),
  :global(select:focus-visible),
  :global(textarea:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  /* Choices with many or dynamic options stay a native <select>, drawn to
     match SegmentedControl's height and corners. */
  :global(select) {
    min-height: var(--control-size);
    padding: var(--space-1) 28px var(--space-1) var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background-color: var(--color-bg);
    background-image:
      linear-gradient(45deg, transparent 50%, var(--color-text-secondary) 50%),
      linear-gradient(135deg, var(--color-text-secondary) 50%, transparent 50%);
    background-position:
      calc(100% - 15px) 50%,
      calc(100% - 11px) 50%;
    background-repeat: no-repeat;
    background-size: 4px 4px;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    appearance: none;
    cursor: pointer;
    transition:
      border-color var(--transition),
      background-color var(--transition);
  }

  :global(select:hover:not(:disabled)) {
    border-color: var(--color-text-tertiary);
  }

  :global(select:disabled) {
    opacity: 0.5;
    cursor: default;
  }

  @media (prefers-reduced-motion: reduce) {
    :global(*),
    :global(*::before),
    :global(*::after) {
      transition: none !important;
      animation: none !important;
      scroll-behavior: auto !important;
    }
  }

  /* One rotation keyframe, shared by every spinner/spinning-icon in the app
     (docs/UNIFIED_UX_MOTION_PLAN.md §4 "대기는 Spinner.svelte 하나만").
     `-global-` keeps Svelte from mangling the name, so Spinner.svelte and
     IconButton.svelte can both reference `animation: spin ...` without
     redefining the keyframe themselves - scripts/check-motion.sh fails the
     build if any other file declares its own copy of it. */
  @keyframes -global-spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* One "something just appeared in place" fade, shared by every inline
     status/confirmation message (SettingsRow, SettingsAi, SettingsStorage)
     instead of each defining its own near-identical keyframe
     (docs/UNIFIED_UX_MOTION_PLAN.md §5.5 결정 1, K3). Fade only - the content
     changes in place, so nothing needs to move to explain it (§4 "내용 교체"). */
  @keyframes -global-fade-in {
    from {
      opacity: 0;
    }
  }

  /* An inline search/filter pill rising into place - shared by PostsFeed's
     post search and the sidebar's feed filter (docs/
     PANEL_DESIGN_UNIFICATION_PLAN.md §5 step 4) instead of each defining its
     own copy of the same keyframe. */
  @keyframes -global-search-in {
    from {
      opacity: 0;
      transform: translateY(var(--motion-rise));
    }
  }

  /* Shared "nothing to show here" treatment, used by the posts list, the
     reader, and the AI panel so every empty/placeholder state in the app
     reads the same way instead of three different ad-hoc layouts. */
  :global(.empty-placeholder) {
    display: flex;
    min-height: 100%;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 32px 16px;
    color: var(--color-text-secondary);
    text-align: center;
  }

  :global(.empty-placeholder-icon) {
    display: grid;
    width: 40px;
    height: 40px;
    place-items: center;
    border-radius: var(--radius-xl);
    background: var(--color-bg-secondary);
    color: var(--color-text-tertiary);
  }

  :global(.empty-placeholder h1),
  :global(.empty-placeholder h3) {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
  }

  :global(.empty-placeholder p) {
    max-width: 280px;
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-body);
    line-height: 1.5;
  }

  /* Shared "are you sure" treatment for a destructive action (unsubscribe,
     delete a folder, ...) - one boxed confirm instead of every component
     inventing its own wording/styling for the same kind of decision. */
  :global(.confirm-row) {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    margin: 2px 0 8px;
    padding: 9px 11px;
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
    color: var(--color-text-secondary);
    font-size: var(--text-body);
    line-height: 1.4;
  }

  :global(.confirm-actions) {
    display: flex;
    flex-shrink: 0;
    gap: 6px;
  }

  :global(.confirm-actions button) {
    min-height: 28px;
    padding: 4px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  :global(.confirm-actions .confirm-remove),
  :global(.confirm-actions .confirm-remove:hover:not(:disabled)) {
    border-color: var(--color-error);
    background: var(--color-error);
    color: var(--color-on-error);
  }

  :global(.confirm-remove:hover:not(:disabled)) {
    filter: brightness(0.92);
  }

  :global(.confirm-remove:disabled) {
    opacity: 0.5;
    cursor: default;
  }

  /* Shared inline error treatment (promoted from AiPanel's original error
     card) - icon + message + optional action, reused anywhere a form or
     panel needs a field-adjacent error instead of a global toast. */
  :global(.error-card) {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    max-height: 130px;
    overflow: auto;
    margin: 0 0 8px;
    padding: 9px;
    border: 1px solid color-mix(in srgb, var(--color-error) 32%, var(--color-border));
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--color-error) 6%, var(--color-bg));
  }

  :global(.error-mark) {
    display: grid;
    width: 17px;
    height: 17px;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--color-error) 15%, transparent);
    color: var(--color-error);
  }

  :global(.error-copy) {
    min-width: 0;
    flex: 1;
  }

  :global(.error-copy strong) {
    display: block;
    margin-bottom: 3px;
    color: var(--color-error);
    font-size: var(--text-small);
  }

  :global(.error-copy p) {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.4;
    overflow-wrap: anywhere;
  }

  :global(.error-card button) {
    flex: 0 0 auto;
    padding: 4px 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  :global(.error-card button:disabled) {
    opacity: 0.5;
    cursor: default;
  }

  :global(.error-card .dismiss-error) {
    display: inline-flex;
    align-items: center;
    padding: 0 4px;
    border-color: transparent;
    background: transparent;
  }
</style>
