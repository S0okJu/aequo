<script lang="ts">
  import SegmentedControl from '../SegmentedControl.svelte';
  import SettingsGroup from './SettingsGroup.svelte';
  import SettingsGrid from './SettingsGrid.svelte';
  import SettingsRow from './SettingsRow.svelte';
  import SettingsUpdates from './SettingsUpdates.svelte';
  import { applyThemePreference, getThemePreference, type ThemePreference } from '$lib/theme';
  import {
    getLocalePreference,
    setLocalePreference,
    t,
    type LocalePreference,
  } from '$lib/i18n/index.svelte';

  let themePreference = $state<ThemePreference>(getThemePreference());

  let changed = $state<'theme' | 'language' | null>(null);
  let statusKey = $state(0);

  function updateLanguage(preference: LocalePreference) {
    setLocalePreference(preference);
    changed = 'language';
    statusKey++;
  }

  function updateTheme(preference: ThemePreference) {
    themePreference = preference;
    applyThemePreference(preference);
    changed = 'theme';
    statusKey++;
  }
</script>

<SettingsGrid>
  <SettingsGroup title={t('settings.group.interface')}>
    <SettingsRow
      label={t('settings.theme')}
      labelId="setting-theme"
      hint={t('settings.themeHint')}
      status={changed === 'theme' ? t('settings.applied') : null}
      {statusKey}
    >
      <SegmentedControl
        size="sm"
        block
        labelledby="setting-theme"
        value={themePreference}
        onChange={updateTheme}
        options={[
          { value: 'system', label: t('settings.theme.system'), icon: 'monitor' },
          { value: 'light', label: t('settings.theme.light'), icon: 'sun' },
          { value: 'dark', label: t('settings.theme.dark'), icon: 'moon' },
        ]}
      />
    </SettingsRow>
    <SettingsRow
      label={t('settings.language')}
      labelId="setting-language"
      hint={t('settings.languageHint')}
      status={changed === 'language' ? t('settings.applied') : null}
      {statusKey}
    >
      <SegmentedControl
        size="sm"
        block
        labelledby="setting-language"
        value={getLocalePreference()}
        onChange={updateLanguage}
        options={[
          { value: 'system', label: t('settings.language.system') },
          { value: 'en', label: t('settings.language.en') },
          { value: 'ko', label: t('settings.language.ko') },
        ]}
      />
    </SettingsRow>
  </SettingsGroup>
  <SettingsUpdates />
</SettingsGrid>
