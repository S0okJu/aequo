import { describe, expect, it } from 'vitest';

import { formatDayHeading, formatRelative, shortFeedTitle } from './format';

const now = Date.UTC(2026, 10, 17, 12, 0, 0) / 1000;

describe('formatRelative', () => {
  it('uses relative units within a day', () => {
    expect(formatRelative(now - 10, now, 'en')).toBe('now');
    expect(formatRelative(now - 5 * 60, now, 'en')).toBe('5 min. ago');
    expect(formatRelative(now - 3 * 3600, now, 'en')).toBe('3 hr. ago');
  });

  it('switches to a short date after a day, adding the year only when it differs', () => {
    expect(formatRelative(now - 2 * 86400, now, 'en')).toBe('Nov 15');
    expect(formatRelative(Date.UTC(2025, 0, 3, 12) / 1000, now, 'en')).toBe('Jan 3, 2025');
  });

  it('follows the requested locale', () => {
    expect(formatRelative(now - 3 * 3600, now, 'ko')).toBe('3시간 전');
  });

  it('treats future timestamps as now', () => {
    expect(formatRelative(now + 600, now, 'en')).toBe('now');
  });
});

describe('formatDayHeading', () => {
  it('omits the year for the current year', () => {
    expect(formatDayHeading(now, now, 'en')).toBe('Tuesday, November 17');
  });
});

describe('shortFeedTitle', () => {
  it('drops a tagline after a spaced separator', () => {
    expect(shortFeedTitle('The New Stack | DevOps, Open Source, and Cloud Native News')).toBe(
      'The New Stack',
    );
    expect(shortFeedTitle('MUSINSA tech - Medium')).toBe('MUSINSA tech');
  });

  it('keeps names without a spaced separator', () => {
    expect(shortFeedTitle('토스 기술 블로그, 토스테크')).toBe('토스 기술 블로그, 토스테크');
    expect(shortFeedTitle('D2-Blog')).toBe('D2-Blog');
  });

  it('falls back when the title is empty', () => {
    expect(shortFeedTitle(null, 'https://example.com/feed')).toBe('https://example.com/feed');
  });
});
