// App language: a persisted preference ('system' follows navigator.language)
// resolved to one of the shipped locales, plus `t()` for UI strings.
// `t()` reads reactive state, so any template or $derived that calls it
// re-renders when the language changes (docs/UI_SIMPLIFICATION_PLAN.md §3.15).

import en, { type Message, type MessageKey } from './en';
import ko from './ko';

export type Locale = 'en' | 'ko';
export type LocalePreference = 'system' | Locale;
export type { MessageKey };

const LOCALE_KEY = 'aequo:locale';
const dictionaries: Record<Locale, Record<MessageKey, Message>> = { en, ko };

function readPreference(): LocalePreference {
  try {
    const value = localStorage.getItem(LOCALE_KEY);
    return value === 'en' || value === 'ko' ? value : 'system';
  } catch {
    return 'system';
  }
}

function systemLocale(): Locale {
  const language = typeof navigator === 'undefined' ? '' : navigator.language;
  return language.toLowerCase().startsWith('ko') ? 'ko' : 'en';
}

const state = $state({ preference: readPreference() as LocalePreference });

export function getLocalePreference(): LocalePreference {
  return state.preference;
}

/** The locale actually in use (the preference with 'system' resolved). */
export function currentLocale(): Locale {
  return state.preference === 'system' ? systemLocale() : state.preference;
}

export function setLocalePreference(preference: LocalePreference): void {
  state.preference = preference;
  try {
    if (preference === 'system') localStorage.removeItem(LOCALE_KEY);
    else localStorage.setItem(LOCALE_KEY, preference);
  } catch {
    // The choice still applies for this session.
  }
  applyDocumentLanguage();
}

/** Keeps <html lang> in step with the UI language (line breaking, fonts, screen readers). */
export function applyDocumentLanguage(): void {
  if (typeof document !== 'undefined') document.documentElement.lang = currentLocale();
}

export function t(key: MessageKey, params: Record<string, string | number> = {}): string {
  const locale = currentLocale();
  const message = dictionaries[locale][key] ?? en[key];
  let text: string;
  if (typeof message === 'string') {
    text = message;
  } else {
    const count = Number(params.count ?? 0);
    const category = new Intl.PluralRules(locale).select(count);
    text = (category === 'one' ? message.one : undefined) ?? message.other;
  }
  return text.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}
