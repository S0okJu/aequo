import { describe, expect, it } from 'vitest';

import en from './en';
import ko from './ko';
import { currentLocale, setLocalePreference, t } from './index.svelte';

describe('i18n', () => {
  it('has a Korean string for every English key', () => {
    expect(Object.keys(ko).sort()).toEqual(Object.keys(en).sort());
  });

  it('fills placeholders and picks plural forms per locale', () => {
    setLocalePreference('en');
    expect(t('toast.markedRead', { count: 1 })).toBe('Marked 1 post as read');
    expect(t('toast.markedRead', { count: 3 })).toBe('Marked 3 posts as read');

    setLocalePreference('ko');
    expect(currentLocale()).toBe('ko');
    expect(t('toast.markedRead', { count: 1 })).toBe('글 1개를 읽음으로 표시했습니다');
    expect(document.documentElement.lang).toBe('ko');
  });

  it('follows the system language when set to system', () => {
    setLocalePreference('system');
    // jsdom reports en-US.
    expect(currentLocale()).toBe('en');
  });
});
