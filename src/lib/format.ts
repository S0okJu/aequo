// Shared display formatting for dates and feed names, so every pane renders
// the same string for the same value (docs/UI_SIMPLIFICATION_PLAN.md §3.1,
// §3.15). `locale` stays optional until the app-language setting lands;
// `undefined` means "the system locale".

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** "just now" / "5 min. ago" / "3 hr. ago" under a day, then "Nov 15" (plus the year when it differs). */
export function formatRelative(
  unixSecs: number,
  nowSecs = Date.now() / 1000,
  locale?: string,
): string {
  const elapsed = Math.max(0, nowSecs - unixSecs);
  if (elapsed < DAY) {
    const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto', style: 'short' });
    if (elapsed < MINUTE) return rtf.format(0, 'second');
    if (elapsed < HOUR) return rtf.format(-Math.floor(elapsed / MINUTE), 'minute');
    return rtf.format(-Math.floor(elapsed / HOUR), 'hour');
  }
  const date = new Date(unixSecs * 1000);
  const sameYear = date.getFullYear() === new Date(nowSecs * 1000).getFullYear();
  return new Intl.DateTimeFormat(locale, {
    month: 'short',
    day: 'numeric',
    ...(sameYear ? {} : { year: 'numeric' }),
  }).format(date);
}

/** Full date and time, for tooltips and the reader header. */
export function formatDateTime(unixSecs: number, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(
    new Date(unixSecs * 1000),
  );
}

/** Day group heading in the post list, e.g. "Tuesday, November 17". */
export function formatDayHeading(
  unixSecs: number,
  nowSecs = Date.now() / 1000,
  locale?: string,
): string {
  const date = new Date(unixSecs * 1000);
  const sameYear = date.getFullYear() === new Date(nowSecs * 1000).getFullYear();
  return new Intl.DateTimeFormat(locale, {
    weekday: 'long',
    month: 'long',
    day: 'numeric',
    ...(sameYear ? {} : { year: 'numeric' }),
  }).format(date);
}

/**
 * Drops the tagline many feeds append to their name ("The New Stack | DevOps,
 * Open Source, …" -> "The New Stack"). Only splits on a spaced separator so
 * names like "D2-Blog" stay intact.
 */
export function shortFeedTitle(title: string | null | undefined, fallback = ''): string {
  const name = (title ?? '').trim();
  if (!name) return fallback;
  const head = name.split(/\s+[|–—-]\s+/)[0].trim();
  return head.length >= 2 ? head : name;
}
