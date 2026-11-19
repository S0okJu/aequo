/* global window, localStorage, document, getComputedStyle */
// Deterministic browser checks; run against `pnpm dev --port 1423`.
import { chromium, expect } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import process from 'node:process';
import presets from '../src/lib/resourcePresets.json' with { type: 'json' };
const browser = await chromium.launch({ headless: true });
const output = '.perf-results/settings-ui';
await mkdir(output, { recursive: true });
try {
  for (const theme of ['light', 'dark']) {
    for (const width of [900, 1100, 1360, 1920]) {
      const page = await browser.newPage({ viewport: { width, height: 600 } });
      if (width === 900) await page.emulateMedia({ reducedMotion: 'reduce' });
      await page.addInitScript(
        ({ theme, presets }) => {
          localStorage.setItem('aequo:theme', theme);
          localStorage.setItem('aequo:locale', 'en');
          let settings = presets.balanced;
          window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
          window.__TAURI_INTERNALS__ = {
            transformCallback: () => 1,
            unregisterCallback() {},
            invoke: async (cmd, args = {}) => {
              // Subscription failures must not prevent unrelated settings from rendering.
              if (cmd === 'list_feeds' || cmd === 'list_feed_folders') {
                if (!window.settingsFeedsAvailable) throw new Error('Feeds unavailable');
                if (cmd === 'list_feed_folders') return [];
                return Array.from({ length: 70 }, (_, index) => ({
                  id: index + 1,
                  title: `Feed ${index + 1}`,
                  url: `https://example.com/${index + 1}`,
                  format: 'rss',
                  intervalSecs: 1800,
                  nextFetchAt: 0,
                  createdAt: 0,
                  unreadCount: 0,
                  folderId: null,
                  hiddenFromAll: false,
                  consecutiveFailures: 0,
                  lastError: null,
                  language: 'en',
                  customTitle: null,
                }));
              }
              if (cmd === 'get_resource_settings')
                return { snapshot: { revision: 1, settings }, runningWork: 0, recoveryError: null };
              if (cmd === 'set_resource_settings') {
                settings = args.settings;
                return { snapshot: { revision: 2, settings }, runningWork: 0, recoveryError: null };
              }
              if (cmd === 'check_ai_providers')
                return [
                  { provider: 'codex', installed: true, version: '1.0', error: null },
                  { provider: 'claude', installed: false, version: null, error: null },
                ];
              if (cmd === 'get_retention_settings')
                return { readRetentionDays: 90, fullTextRetentionDays: 30 };
              if (cmd === 'preview_retention_cleanup')
                return { articlesToRemove: 3, fullTextsToClear: 2, estimatedBytesToFree: 2048 };
              return null;
            },
          };
        },
        { theme, presets },
      );
      await page.goto(`${process.env.SETTINGS_UI_URL ?? 'http://127.0.0.1:1423'}/settings`);
      await expect(page.getByRole('radiogroup', { name: 'Theme' })).toBeVisible();
      await expect(page.getByRole('alert')).toHaveCount(0);
      await expect(page.getByRole('heading', { level: 1 })).toHaveCount(1);
      const otherTheme = theme === 'light' ? 'Dark' : 'Light';
      await page
        .getByRole('radiogroup', { name: 'Theme', exact: true })
        .getByRole('radio', { name: otherTheme, exact: true })
        .click();
      await expect(page.getByText('Applied', { exact: true })).toBeVisible();
      await page
        .getByRole('radiogroup', { name: 'Theme', exact: true })
        .getByRole('radio', { name: theme === 'light' ? 'Light' : 'Dark', exact: true })
        .click();
      await page.getByRole('radio', { name: '한국어', exact: true }).click();
      await expect(page.getByRole('heading', { name: '일반', exact: true })).toBeVisible();
      await page.screenshot({
        animations: 'disabled',
        path: `${output}/${theme}-${width}-general-ko.png`,
      });
      await page.getByRole('radio', { name: 'English', exact: true }).click();
      await page.getByRole('link', { name: 'AI', exact: true }).click();
      await page.getByRole('combobox', { name: 'AI for new conversations' }).selectOption('claude');
      await page.getByRole('link', { name: 'Performance', exact: true }).click();
      await expect(page.getByRole('radiogroup', { name: 'Theme' })).toHaveCount(0);
      await expect(page.getByRole('combobox', { name: 'AI for new conversations' })).toHaveCount(0);
      const select = page.getByLabel('Concurrent feed updates');
      await expect(select).toBeVisible();
      const size = await select.boundingBox();
      expect(size.width).toBeGreaterThanOrEqual(180);
      expect(size.height).toBeGreaterThanOrEqual(32);
      // The page pane scrolls at the window edge; the form has no inner scroller.
      expect(
        await page
          .locator('.settings-content')
          .evaluate((el) => Math.abs(el.getBoundingClientRect().right - window.innerWidth) < 2),
      ).toBe(true);
      expect(
        await page
          .locator('.section-body')
          .first()
          .evaluate((el) => getComputedStyle(el).overflowY),
      ).toBe('visible');
      await page.getByRole('button', { name: 'Restore defaults' }).scrollIntoViewIfNeeded();
      expect(
        await page.locator('.settings-content').evaluate((el) => el.scrollTop),
      ).toBeGreaterThan(0);
      await page.getByRole('link', { name: 'AI', exact: true }).click();
      const toggle = page.locator('#ai-auto-open');
      await expect(toggle).toBeVisible();
      await expect(page.getByRole('combobox', { name: 'AI for new conversations' })).toHaveValue(
        'claude',
      );
      await expect(page.getByRole('link', { name: 'AI', exact: true })).toHaveAttribute(
        'aria-current',
        'page',
      );
      await toggle.focus();
      const checked = await toggle.isChecked();
      await page.keyboard.press('Space');
      expect(await toggle.isChecked()).toBe(!checked);
      await page.getByRole('link', { name: 'Storage', exact: true }).click();
      await expect(page.getByText('Unread and saved posts are always kept.')).toBeVisible();
      // Four- and five-option settings use the same shared 32px control.
      const retentionGroups = page.locator('.preference-section:not([hidden]) [role="radiogroup"]');
      const geometries = await retentionGroups.evaluateAll((groups) =>
        groups.map((group) => ({
          height: group.getBoundingClientRect().height,
          width: group.getBoundingClientRect().width,
          buttons: Array.from(group.querySelectorAll('button')).map((button) => ({
            height: button.getBoundingClientRect().height,
            width: button.getBoundingClientRect().width,
            fits: button.scrollWidth <= button.clientWidth,
            whiteSpace: getComputedStyle(button).whiteSpace,
          })),
        })),
      );
      expect(geometries).toHaveLength(2);
      expect(geometries[0].height).toBe(32);
      expect(geometries[1].height).toBe(32);
      expect(geometries[0].width).toBe(geometries[1].width);
      for (const group of geometries) {
        for (const button of group.buttons) {
          expect(button.height).toBe(28);
          expect(button.fits).toBe(true);
          expect(button.whiteSpace).toBe('nowrap');
          expect(Math.abs(button.width - group.buttons[0].width)).toBeLessThan(1);
        }
      }

      await page.screenshot({
        animations: 'disabled',
        path: `${output}/${theme}-${width}-storage.png`,
      });
      expect(
        await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
      ).toBe(true);
      expect(
        await page
          .locator('.section-body')
          .first()
          .evaluate((el) => el.scrollWidth <= el.clientWidth),
      ).toBe(true);
      await page.getByRole('link', { name: 'Subscriptions', exact: true }).click();
      await expect(page.getByRole('alert')).toContainText('Feeds unavailable');
      await page.evaluate(() => {
        window.settingsFeedsAvailable = true;
      });
      await page.getByRole('button', { name: 'Try again' }).click();
      const list = page.locator('.list');
      await expect(page.locator('[data-feed-row="1"]').first()).toBeVisible();
      expect(await page.locator('.settings-content').evaluate((el) => el.scrollTop)).toBe(0);
      expect(await list.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
      await page.locator('[data-feed-row="1"]').first().click();
      const detail = page.locator('.detail-body');
      await expect(detail).toBeVisible();
      expect(
        await detail.evaluate(
          (el) => Math.abs(el.getBoundingClientRect().right - window.innerWidth) < 2,
        ),
      ).toBe(true);
      await list.evaluate((el) => {
        el.scrollTop = 400;
      });
      expect(await detail.evaluate((el) => el.scrollTop)).toBe(0);
      await page.screenshot({
        animations: 'disabled',
        path: `${output}/${theme}-${width}-subscriptions.png`,
      });

      if (width === 1360) {
        const input = page.locator('.detail input[type="text"]').first();
        await input.fill('Unsubmitted resize draft');
        await input.focus();
        for (const resizedWidth of [1920, 1100, 900, 760, 1360, 1920]) {
          await page.setViewportSize({ width: resizedWidth, height: 600 });
          await expect(input).toBeFocused();
          await expect(input).toHaveValue('Unsubmitted resize draft');
          expect(
            await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
          ).toBe(true);
          await expect(page.locator('.detail')).toBeInViewport();
        }
        await page.setViewportSize({ width, height: 600 });
      }
      await page.getByRole('link', { name: 'General', exact: true }).click();
      await expect(page.getByRole('radiogroup', { name: 'Theme' })).toBeVisible();
      await expect(page.getByRole('alert')).toHaveCount(0);
      await page.goto(
        `${process.env.SETTINGS_UI_URL ?? 'http://127.0.0.1:1423'}/settings?section=storage`,
      );
      await expect(page.getByRole('heading', { name: 'Storage', exact: true })).toBeInViewport();
      await expect(page.getByRole('link', { name: 'Storage', exact: true })).toHaveAttribute(
        'aria-current',
        'page',
      );
      if (width === 1360) {
        await page.getByRole('link', { name: 'Performance', exact: true }).click();
        const field = page.getByLabel('Concurrent feed updates');
        await field.focus();
        const row = field.locator('xpath=../..');
        const before = await row.evaluate((el) => el.getBoundingClientRect().top);
        const value = await field.inputValue();
        for (const resizedWidth of [1920, 1100, 900, 760, 1360, 1920]) {
          await page.setViewportSize({ width: resizedWidth, height: 600 });
          await expect(field).toBeFocused();
          await expect(field).toHaveValue(value);
          await expect(field).toBeInViewport();
          await expect
            .poll(async () =>
              Math.abs((await row.evaluate((el) => el.getBoundingClientRect().top)) - before),
            )
            .toBeLessThan(4);
          expect(
            await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
          ).toBe(true);
        }
        await page.setViewportSize({ width, height: 600 });
      }
      const selectedCategory = await page
        .locator('.settings-nav [aria-current=page]')
        .textContent();
      const paneBox = await page.locator('.settings-content').boundingBox();
      await page.mouse.move(paneBox.x + paneBox.width / 2, paneBox.y + 40);
      await page.mouse.wheel(0, -10000);
      await expect(page.locator('.settings-nav [aria-current=page]')).toHaveText(selectedCategory);
      await page.getByRole('link', { name: 'General', exact: true }).click();
      await expect(page.getByRole('radiogroup', { name: 'Theme' })).toBeVisible();
      await expect(page.getByRole('heading', { level: 1 })).toHaveCount(1);
      await page.close();
    }
  }
  process.stdout.write('Settings UI: 8 theme/viewport combinations passed.\n');
} finally {
  await browser.close();
}
