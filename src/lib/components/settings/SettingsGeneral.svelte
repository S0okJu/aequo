<script lang="ts">
  import SettingsRow from './SettingsRow.svelte';
  import { applyThemePreference, getThemePreference, type ThemePreference } from '$lib/theme';
  import {
    getLocalePreference,
    setLocalePreference,
    t,
    type LocalePreference,
  } from '$lib/i18n/index.svelte';

  let themePreference = $state<ThemePreference>(getThemePreference());

  function updateTheme(preference: ThemePreference) {
    themePreference = preference;
    applyThemePreference(preference);
  }
</script>

<SettingsRow label={t('settings.theme')} for="setting-theme" hint={t('settings.themeHint')}>
  <select
    id="setting-theme"
    value={themePreference}
    onchange={(event) => updateTheme(event.currentTarget.value as ThemePreference)}
  >
    <option value="system">{t('settings.theme.system')}</option>
    <option value="light">{t('settings.theme.light')}</option>
    <option value="dark">{t('settings.theme.dark')}</option>
  </select>
</SettingsRow>

<SettingsRow
  label={t('settings.language')}
  for="setting-language"
  hint={t('settings.languageHint')}
>
  <select
    id="setting-language"
    value={getLocalePreference()}
    onchange={(event) => setLocalePreference(event.currentTarget.value as LocalePreference)}
  >
    <option value="system">{t('settings.language.system')}</option>
    <option value="en">{t('settings.language.en')}</option>
    <option value="ko">{t('settings.language.ko')}</option>
  </select>
</SettingsRow>
