/**
 * Hand-maintained icon dictionary: name -> inner <svg> markup.
 * Default stroke is `currentColor` (set on the wrapping <svg> by Icon.svelte)
 * so every icon inherits whatever text/accent/error color its context uses,
 * light or dark mode, with no per-icon color prop needed.
 */
export const ICONS: Record<string, string> = {
  close: '<path d="M6 6l12 12M18 6 6 18"/>',

  'external-link':
    '<path d="M10 6H6a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4"/><path d="M14 4h6v6"/><path d="M20 4 11 13"/>',

  star: '<path d="M12 3.5l2.6 5.6 6.1.6-4.6 4.2 1.3 6-5.4-3.1-5.4 3.1 1.3-6-4.6-4.2 6.1-.6z" fill="currentColor" stroke="none"/>',

  'star-outline':
    '<path d="M12 3.5l2.6 5.6 6.1.6-4.6 4.2 1.3 6-5.4-3.1-5.4 3.1 1.3-6-4.6-4.2 6.1-.6z"/>',

  send: '<path d="M3 11l18-8-8 18-2-8-8-2Z" fill="currentColor" stroke="none"/>',

  settings:
    '<circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1"/>',

  'chevron-left': '<path d="M15 5 8 12l7 7"/>',
  'chevron-right': '<path d="M9 5l7 7-7 7"/>',
  'chevron-down': '<path d="M5 9l7 7 7-7"/>',

  retry: '<path d="M20 11A8 8 0 1 0 19 16"/><path d="M20 4v7h-7"/>',

  error:
    '<circle cx="12" cy="12" r="9"/><path d="M12 7v6"/><path d="M12 16.5h.01" stroke-width="2.5"/>',

  success: '<circle cx="12" cy="12" r="9"/><path d="M8 12.5l2.5 2.5L16 9.5"/>',

  info: '<circle cx="12" cy="12" r="9"/><path d="M12 8h.01" stroke-width="2.5"/><path d="M11 12h1v5h1"/>',

  memo: '<path d="M5 4h11l3 3v13H5z"/><path d="M16 4v3h3"/><path d="M8 11h8M8 15h5"/>',

  'ask-ai':
    '<path d="M12 2l1.8 6.2L20 10l-6.2 1.8L12 18l-1.8-6.2L4 10l6.2-1.8Z" fill="currentColor" stroke="none"/>',

  edit: '<path d="M4 20h4L19 9l-4-4L4 16Z"/><path d="M14 6l4 4"/>',

  more: '<circle cx="12" cy="5" r="1.2" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.2" fill="currentColor" stroke="none"/><circle cx="12" cy="19" r="1.2" fill="currentColor" stroke="none"/>',

  empty:
    '<path d="M4 13V7a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v6"/><path d="M4 13l3 5h10l3-5"/><path d="M4 13h5l1 2h4l1-2h5"/>',

  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/>',

  'check-all': '<path d="M3 12.5l4 4L15 8"/><path d="M11 16.5l1 1L21 8"/>',

  inbox:
    '<path d="M4 13l2.5-7h11L20 13v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1Z"/><path d="M4 13h4.5l1 2h5l1-2H20"/>',

  plus: '<path d="M12 5v14M5 12h14"/>',

  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>',

  search: '<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2"/>',

  dot: '<circle cx="12" cy="12" r="4" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="8.5"/>',

  check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',

  stop: '<rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" stroke="none"/>',
};

export type IconName = keyof typeof ICONS;
