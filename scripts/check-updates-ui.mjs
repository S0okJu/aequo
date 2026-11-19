/* global window, localStorage, document */
import { chromium, expect } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import process from 'node:process';
import presets from '../src/lib/resourcePresets.json' with { type: 'json' };

const browser = await chromium.launch({ headless: true });
const output = '.perf-results/updates-ui';
await mkdir(output, { recursive: true });
try {
  for (const theme of ['light', 'dark']) {
    for (const locale of ['en', 'ko']) {
      const page = await browser.newPage({ viewport: { width: 1100, height: 700 } });
      const errors = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.addInitScript(
        ({ theme, locale, presets }) => {
          localStorage.setItem('aequo:theme', theme);
          localStorage.setItem('aequo:locale', locale);
          let state = {
            revision: 1,
            operationId: 1,
            currentVersion: '1.0.1',
            phase: 'available',
            autoUpdate: false,
            enabled: true,
            lastSuccessfulCheckAt: 1795080000,
            release: {
              version: '1.0.2',
              notes: 'Improved updates.\n업데이트 안정성을 개선했습니다.',
              publishedAt: '2026-11-19T00:00:00Z',
              releaseUrl: 'https://github.com/S0okJu/aequo/releases/tag/v1.0.2',
            },
            downloadedBytes: 0,
            totalBytes: null,
            error: null,
            appliedVersion: null,
            pendingInstalledVersion: null,
          };
          window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
          window.__TAURI_INTERNALS__ = {
            transformCallback: () => 1,
            unregisterCallback() {},
            invoke: async (command, args = {}) => {
              if (command === 'get_update_state') return state;
              if (command === 'set_update_settings')
                return (state = {
                  ...state,
                  revision: state.revision + 1,
                  autoUpdate: args.autoUpdate,
                });
              if (command === 'install_available_update')
                return (state = {
                  ...state,
                  revision: state.revision + 1,
                  phase: 'restart_required',
                  pendingInstalledVersion: '1.0.2',
                });
              if (command === 'list_feeds' || command === 'list_feed_folders') return [];
              if (command === 'check_ai_providers') return [];
              if (command === 'get_resource_settings')
                return {
                  snapshot: { revision: 1, settings: presets.balanced },
                  runningWork: 0,
                  recoveryError: null,
                };
              if (command === 'get_retention_settings')
                return { readRetentionDays: 365, fullTextRetentionDays: 180 };
              if (command === 'preview_retention_cleanup')
                return { articlesToRemove: 0, fullTextsToClear: 0, estimatedBytesToFree: 0 };
              return null;
            },
          };
        },
        { theme, locale, presets },
      );
      await page.goto(`${process.env.SETTINGS_UI_URL ?? 'http://127.0.0.1:1423'}/settings`);
      const install = page.getByRole('button', {
        name: locale === 'en' ? 'Install update' : '업데이트 설치',
        exact: true,
      });
      await expect(install).toBeVisible();
      await install.scrollIntoViewIfNeeded();
      await page.screenshot({
        path: `${output}/${theme}-${locale}-available.png`,
        animations: 'disabled',
      });
      await install.click();
      const restart = page.getByRole('button', {
        name: locale === 'en' ? 'Restart to apply' : '재시작하여 적용',
        exact: true,
      });
      await expect(restart).toBeVisible();
      await restart.scrollIntoViewIfNeeded();
      await page.screenshot({
        path: `${output}/${theme}-${locale}-restart.png`,
        animations: 'disabled',
      });
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
      ).toBe(true);
      expect(errors).toEqual([]);
      await page.close();
    }
  }
} finally {
  await browser.close();
}
