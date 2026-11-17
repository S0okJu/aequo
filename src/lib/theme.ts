export type ThemePreference = 'system' | 'light' | 'dark';

const THEME_KEY = 'aequo:theme';

export function getThemePreference(): ThemePreference {
  if (typeof localStorage === 'undefined') return 'system';

  try {
    const value = localStorage.getItem(THEME_KEY);
    return value === 'light' || value === 'dark' ? value : 'system';
  } catch {
    return 'system';
  }
}

export function applyThemePreference(preference: ThemePreference): void {
  if (typeof document !== 'undefined') {
    if (preference === 'system') {
      document.documentElement.removeAttribute('data-theme');
    } else {
      document.documentElement.dataset.theme = preference;
    }
  }

  if (typeof localStorage !== 'undefined') {
    try {
      if (preference === 'system') {
        localStorage.removeItem(THEME_KEY);
      } else {
        localStorage.setItem(THEME_KEY, preference);
      }
    } catch {
      // Keep the current selection active for this session if storage is unavailable.
    }
  }
}
