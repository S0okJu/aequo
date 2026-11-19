/* global window, document, localStorage, getComputedStyle, console */
import process from 'node:process';
// Browser integration checks with deterministic Tauri IPC fixtures.
// Run `pnpm dev --port 1423` first. Real provider execution is checked separately in Rust.
import { chromium, expect } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
const baseURL = process.env.AI_CONTEXT_UI_URL ?? 'http://127.0.0.1:1423';
const output = process.env.AI_CONTEXT_UI_OUTPUT ?? '.perf-results/ai-context-ui';
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ headless: true });
const results = [];
async function fixture(page, locale = 'en') {
  await page.addInitScript((locale) => {
    localStorage.setItem('aequo:locale', locale);
    localStorage.setItem('aequo:aiPanelOpen', 'true');
    const now = Math.floor(Date.now() / 1000);
    const feeds = [
      {
        id: 1,
        url: 'https://example.com/feed',
        title: 'Research',
        format: 'rss',
        intervalSecs: 1800,
        nextFetchAt: now,
        createdAt: now,
        unreadCount: 25,
        folderId: null,
        hiddenFromAll: false,
        consecutiveFailures: 0,
        lastError: null,
        language: 'en',
        customTitle: null,
      },
    ];
    const html =
      '<h2>Reading and research</h2><p>Related articles can be compared in a conversation while the reader remains open. The source material should stay visible and the conversation should follow the user across articles.</p>'.repeat(
        12,
      );
    const items = Array.from({ length: 25 }, (_, i) => ({
      id: i + 1,
      feedId: 1,
      guidHash: String(i),
      title:
        locale === 'ko'
          ? `보안 행사와 업계 동향: 웨비나·조찬세미나·골프 마스터즈 일정과 핵심 내용 ${i + 1}`
          : `Article ${i + 1}: Reading across sources`,
      link: `https://example.com/${i + 1}`,
      preview: 'Independent article context',
      publishedAt: now - i * 3600,
      fetchedAt: now,
      isRead: false,
      readAt: null,
      isSaved: false,
    }));
    window.aiAddItem = () =>
      items.unshift({
        ...items[0],
        id: 26,
        guidHash: 'new-arrival',
        title: 'Just arrived before send',
        publishedAt: now + 1,
      });
    const callbacks = new Map();
    const listeners = new Map();
    const sessions = [];
    let next = 1;
    window.aiCalls = [];
    window.aiEmit = (payload) => {
      for (const id of listeners.get('ai-stream') ?? [])
        callbacks.get(id)?.({ event: 'ai-stream', id: 1, payload });
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      transformCallback: (callback) => {
        const id = next++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback: (id) => callbacks.delete(id),
      invoke: async (cmd, args = {}) => {
        window.aiCalls.push({ cmd, args });
        if (cmd === 'plugin:event|listen') {
          const ids = listeners.get(args.event) ?? [];
          ids.push(args.handler);
          listeners.set(args.event, ids);
          return args.handler;
        }
        if (cmd === 'plugin:event|unlisten') return null;
        if (cmd === 'list_feeds') return feeds;
        if (cmd === 'list_feed_folders') return [];
        if (cmd === 'get_feed_icons') return {};
        if (cmd === 'list_items')
          return items
            .filter(
              (i) =>
                (!args.filter?.savedOnly || i.isSaved) &&
                (!args.filter?.query ||
                  `${i.title} ${i.preview}`
                    .toLowerCase()
                    .includes(args.filter.query.toLowerCase())),
            )
            .slice(0, args.limit ?? 25);
        if (cmd === 'count_unread_items') return 25;
        if (cmd === 'get_item_summary') return items.find((i) => i.id === args.itemId) ?? null;
        if (cmd === 'load_article')
          return {
            itemId: args.itemId,
            originalUrl: 'https://example.com',
            finalUrl: null,
            title: null,
            contentHtml: html,
            rssContentHtml: html,
            cached: true,
            fetchedAt: now,
            error: null,
          };
        if (cmd === 'list_item_annotations') return [];
        if (cmd === 'get_resource_settings')
          return {
            snapshot: {
              revision: 1,
              settings: {
                schemaVersion: 1,
                mode: 'light',
                feedConcurrency: 1,
                prefetchFullText: false,
                articleConcurrency: 1,
                minAutoRefreshSecs: 1800,
                pauseOptionalWhenHidden: true,
                retainedPages: 2,
                cacheBudgetMib: 16,
                aiFlushIntervalMs: 100,
              },
            },
            recoveryError: null,
            runningWork: 0,
          };
        if (cmd === 'list_ai_sessions')
          return sessions.filter(
            (s) =>
              s.provider === args.provider && s.contextItems.some((i) => i.itemId === args.itemId),
          );
        if (cmd === 'list_all_ai_sessions') return sessions;
        if (cmd === 'load_ai_transcript')
          return { available: true, messages: [], note: null, hasOlder: false, hasNewer: false };
        if (cmd === 'start_ai_message') {
          const id = args.sessionId ?? sessions.length + 1;
          if (!args.sessionId)
            sessions.push({
              id,
              itemId: null,
              provider: args.provider,
              providerSessionId: 'fixture',
              title: args.prompt,
              createdAt: now,
              updatedAt: now,
              scopeLabel: args.scopeLabel,
              contextItems: args.contextItemIds.map((id, position) => ({
                itemId: id,
                itemTitle: items.find((i) => i.id === id).title,
                feedId: 1,
                position,
              })),
            });
          return { requestId: 'run-' + id, sessionId: id };
        }
        if (cmd === 'add_ai_context_items') {
          const session = sessions.find((s) => s.id === args.sessionId);
          for (const id of args.itemIds)
            if (!session.contextItems.some((i) => i.itemId === id))
              session.contextItems.push({
                itemId: id,
                itemTitle: items.find((i) => i.id === id).title,
                feedId: 1,
                position: session.contextItems.length,
              });
          return session;
        }
        if (cmd === 'remove_ai_context_item') {
          const s = sessions.find((s) => s.id === args.sessionId);
          s.contextItems = s.contextItems.filter((i) => i.position !== args.position);
          return s;
        }
        if (cmd === 'check_ai_providers')
          return ['codex', 'claude'].map((provider) => ({
            provider,
            installed: true,
            version: 'fixture',
            error: null,
          }));
        return null;
      },
    };
  }, locale);
}
// Focused regression for the removed article-selection UI.
if (process.argv.includes('--post-opening')) {
  try {
    const page = await browser.newPage({ viewport: { width: 1360, height: 850 } });
    await fixture(page);
    await page.goto(baseURL);
    const post = page.locator('[data-post-id="1"]');
    await expect(post).toBeVisible();
    await expect(page.locator('.post input[type="checkbox"]')).toHaveCount(0);
    await post.hover();
    await expect(post.locator('.feed-icon')).toHaveCSS('opacity', '1');
    await post.click();
    await expect(page.locator('.reader h1')).toHaveText('Article 1: Reading across sources');
    await page.locator('[data-post-id="2"]').press('Space');
    await expect(page.locator('.reader h1')).toHaveText('Article 2: Reading across sources');
    await page.locator('[data-post-id="3"]').click({ modifiers: ['Shift'] });
    await expect(page.locator('.reader h1')).toHaveText('Article 3: Reading across sources');
    await expect(page.locator('.selection-bar')).toHaveCount(0);
    await page.screenshot({ path: `${output}/post-opening.png` });
    console.log(
      'Passed article opening, persistent feed icons, and removal of selection controls.',
    );
  } finally {
    await browser.close();
  }
  process.exit(0);
}

try {
  for (const width of [1100, 1360, 1920])
    for (const theme of ['light', 'dark'])
      for (const motion of ['no-preference', 'reduce']) {
        const page = await browser.newPage({
          viewport: { width, height: 850 },
          colorScheme: theme,
          reducedMotion: motion,
        });
        const errors = [];
        page.on('pageerror', (e) => errors.push(e.message));
        await fixture(page);
        await page.goto(baseURL);
        await page.locator('.post').first().click();
        await expect(page.locator('.reader h1')).toBeVisible();
        await expect(page.getByLabel('Message the AI assistant')).toBeVisible();
        const drawer = page.locator('.ai-pane.ai-drawer');
        if (width === 1100) {
          await expect(drawer).toBeVisible();
          await page.keyboard.press('Escape');
          await expect(page.locator('.ai-pane')).toBeHidden();
          await page.getByRole('button', { name: 'Ask AI about this post', exact: true }).click();
          await expect(drawer).toBeVisible();
          await page.locator('.reader-pane').click({ position: { x: 30, y: 200 } });
          await expect(page.locator('.ai-pane')).toBeHidden();
          await page.getByRole('button', { name: 'Ask AI about this post', exact: true }).click();
        } else await expect(drawer).toHaveCount(0);
        // Measure after actual layout/drawer animations settle, including delayed first paint.
        await page.waitForTimeout(260);
        await page
          .locator('.layout')
          .evaluate((el) =>
            Promise.all(el.getAnimations().map((animation) => animation.finished.catch(() => {}))),
          );
        if (width === 1100)
          await drawer.evaluate((el) =>
            Promise.all(el.getAnimations().map((animation) => animation.finished.catch(() => {}))),
          );
        const geometry = await page.evaluate(() =>
          Object.fromEntries(
            ['.reader-pane', '.ai-pane', '.posts-pane'].map((selector) => {
              const el = document.querySelector(selector);
              const r = el.getBoundingClientRect();
              return [
                selector,
                {
                  x: r.x,
                  right: r.right,
                  width: r.width,
                  client: el.clientWidth,
                  scroll: el.scrollWidth,
                },
              ];
            }),
          ),
        );
        expect(geometry['.reader-pane'].width).toBeGreaterThanOrEqual(320);
        for (const box of Object.values(geometry)) {
          expect(box.x).toBeGreaterThanOrEqual(0);
          expect(box.right).toBeLessThanOrEqual(width + 1);
          expect(box.scroll).toBeLessThanOrEqual(box.client + 1);
        }
        if (motion === 'reduce')
          expect(
            await page
              .locator('.ai-pane')
              .evaluate((el) => parseFloat(getComputedStyle(el).animationDuration)),
          ).toBeLessThanOrEqual(0.001);
        await page.screenshot({ path: `${output}/${width}-${theme}-${motion}.png` });
        results.push({ width, theme, motion, geometry, errors });
        expect(errors).toEqual([]);
        console.log(`Passed ${width} ${theme} ${motion}`);
        await page.close();
      }
  const page = await browser.newPage({ viewport: { width: 1920, height: 850 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await fixture(page);
  await page.goto(baseURL);
  // Article selection checkboxes are removed; hover keeps the feed identity.
  await expect(page.locator('.post input[type="checkbox"]')).toHaveCount(0);
  const post = page.locator('[data-post-id="1"]');
  await post.hover();
  await expect(post.locator('.feed-icon')).toHaveCSS('opacity', '1');
  await post.click();
  await expect(page.locator('.reader h1')).toHaveText('Article 1: Reading across sources');
  await page.getByRole('button', { name: 'Summarize this view', exact: true }).click();
  await page.getByLabel('Message the AI assistant').fill('Explain this source');
  await page.getByLabel('Message the AI assistant').press('Enter');
  const first = await page.evaluate(
    () => window.aiCalls.find((c) => c.cmd === 'start_ai_message').args,
  );
  expect(first.contextItemIds).toEqual(Array.from({ length: 20 }, (_, index) => index + 1));
  // Closing the panel must preserve its active stream listener and draft transcript.
  await page
    .locator('.ai-panel')
    .getByRole('button', { name: 'Close AI panel', exact: true })
    .click();
  await page.evaluate(() =>
    window.aiEmit({
      requestId: 'run-1',
      sessionId: 1,
      kind: 'delta',
      text: 'Cross-source answer [1]',
    }),
  );
  await page.locator('.posts-scroll').evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await page.locator('[data-post-id="25"]').click();
  await page.getByRole('button', { name: 'Ask AI about this post', exact: true }).click();
  await expect(
    page.locator('.message-content p').filter({ hasText: 'Cross-source answer' }),
  ).toBeVisible();
  await page.evaluate(() =>
    window.aiEmit({
      requestId: 'run-1',
      sessionId: 1,
      kind: 'done',
      text: 'Cross-source answer [1]',
    }),
  );
  await page.getByRole('button', { name: 'Add to this chat', exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'add_ai_context_items').length),
    )
    .toBe(1);
  await page.locator('.citation').first().click();
  await expect(page.locator('.reader h1')).toHaveText('Article 1: Reading across sources');
  await page
    .locator('.ai-panel')
    .getByRole('button', { name: 'Close AI panel', exact: true })
    .click();
  await expect(page.locator('.reader-ai-toggle.has-dot')).toBeVisible();
  await page.locator('.reader-ai-toggle').click();
  // View scope freezes all IDs; the backend caps it at 20.
  await page.getByRole('button', { name: 'Summarize this view', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Group by topic', exact: true })).toBeVisible();
  await page.evaluate(() => window.aiAddItem());
  await page.getByRole('button', { name: 'Group by topic', exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
    )
    .toBe(2);
  const scope = await page.evaluate(
    () => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').at(-1).args,
  );
  expect(scope.contextItemIds).toHaveLength(20);
  expect(scope.contextItemIds[0]).toBe(26);
  expect(scope.scopeLabel).toContain('All');
  await page.evaluate(() =>
    window.aiEmit({ requestId: 'run-2', sessionId: 2, kind: 'done', text: 'Grouped answer' }),
  );
  await page.waitForTimeout(260);
  await page.screenshot({ path: `${output}/multi-article.png` });
  // Space policy persists in settings and closes AI for each newly opened article.
  await page.goto(`${baseURL}/settings?section=ai`);
  // Settings route uses its navigation control, independent of the query string.
  const toggle = page.getByRole('switch', { name: 'Open AI panel automatically', exact: true });
  await expect(toggle).toBeVisible();
  await toggle.uncheck();
  expect(await page.evaluate(() => localStorage.getItem('aequo:aiAutoOpen'))).toBe('false');
  await page.goto(baseURL);
  await page.locator('.post').first().click();
  await expect(page.locator('.ai-pane')).toBeHidden();
  await page.getByRole('button', { name: /Ask AI about this post/ }).click();
  await expect(page.getByLabel('Message the AI assistant')).toBeVisible();
  await page.locator('[data-post-id="2"]').click();
  await expect(page.locator('.ai-pane')).toBeHidden();
  expect(errors).toEqual([]);
  results.push({
    scenario:
      'article opening without checkboxes, stream while closed, add context, citation, frozen view, auto-open setting',
    passed: true,
  });
  await page.close();
  // Korean task labels, reference management, and summary entry from a closed panel.
  for (const width of [1100, 1360]) {
    const page = await browser.newPage({ viewport: { width, height: 850 }, colorScheme: 'light' });
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await fixture(page);
    await page.addInitScript(() => {
      localStorage.setItem('aequo:locale', 'ko');
      localStorage.setItem('aequo:aiPanelOpen', 'false');
    });
    await page.goto(baseURL);
    await page.locator('.post').first().click();
    await expect(page.locator('.ai-pane')).toBeHidden();
    await page
      .locator('.reader-pane')
      .getByRole('button', { name: '핵심 요약', exact: true })
      .click();
    await expect(page.getByRole('heading', { name: '글 도우미', exact: true })).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
      )
      .toBe(1);
    const first = await page.evaluate(
      () => window.aiCalls.find((c) => c.cmd === 'start_ai_message').args,
    );
    expect(first.contextItemIds).toEqual([1]);
    expect(first.sessionId).toBeNull();
    expect(first.prompt).toContain('Summarize this post');
    await expect(page.getByText('답변 작성 중…', { exact: true })).toBeVisible();
    await expect(
      page.locator('.reader-pane').getByRole('button', { name: '핵심 요약', exact: true }),
    ).toBeDisabled();
    await expect(page.getByRole('button', { name: '응답 중지', exact: true })).toHaveText('중지');
    await page.evaluate(() =>
      window.aiEmit({
        requestId: 'run-1',
        sessionId: 1,
        kind: 'done',
        text: '첫 글의 핵심 요약입니다.',
      }),
    );
    await expect(page.getByText('첫 글의 핵심 요약입니다.', { exact: true })).toBeVisible();
    if (width === 1100) await page.keyboard.press('Escape');
    await page.locator('[data-post-id="2"]').click();
    if (width === 1100) await page.keyboard.press('Escape');
    await page
      .locator('.reader-pane')
      .getByRole('button', { name: '핵심 요약', exact: true })
      .click();
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
      )
      .toBe(2);
    const second = await page.evaluate(
      () => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').at(-1).args,
    );
    expect(second.contextItemIds).toEqual([2]);
    expect(second.sessionId).toBeNull();
    await page.evaluate(() =>
      window.aiEmit({
        requestId: 'run-2',
        sessionId: 2,
        kind: 'done',
        text: '두 번째 글의 핵심 요약입니다.',
      }),
    );
    const references = page.getByRole('button', { name: /참고 중인 글 1개/ });
    await references.click();
    const sources = page.getByRole('dialog', { name: '참고 글 확인 및 관리' });
    await expect(sources).toBeVisible();
    const box = await sources.boundingBox();
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(width);
    await references.press('Escape');
    await expect(sources).toBeHidden();
    await expect(page.locator('.ai-pane')).toBeVisible();
    await page.getByRole('button', { name: '이전 대화', exact: true }).click();
    await expect(page.getByRole('dialog', { name: '이전 대화' })).toBeVisible();
    await page.getByRole('button', { name: '이전 대화', exact: true }).click();
    await expect(page.getByRole('dialog', { name: '이전 대화' })).toBeHidden();
    await page.getByRole('combobox', { name: 'AI 변경' }).selectOption('claude');
    await expect(page.getByRole('combobox', { name: 'AI 변경' })).toHaveValue('claude');
    await expect(page.getByRole('heading', { name: '이 글을 어떻게 도와드릴까요?' })).toBeVisible();
    await page.waitForTimeout(260);
    await page.screenshot({ path: `${output}/${width}-ko-summary.png` });
    await page.locator('.ai-panel').screenshot({ path: `${output}/${width}-ko-panel.png` });
    expect(errors).toEqual([]);
    results.push({
      scenario: 'Korean summary entry, fresh context, references, history and provider settings',
      width,
      passed: true,
    });
    await page.close();
  }
  // Minimum supported window height: controls remain reachable as messages scroll.
  for (const locale of ['ko', 'en']) {
    const page = await browser.newPage({ viewport: { width: 1100, height: 600 } });
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await fixture(page);
    await page.addInitScript((locale) => localStorage.setItem('aequo:locale', locale), locale);
    await page.goto(baseURL);
    await page.locator('.post').first().click();
    const picker = page.getByRole('combobox', { name: locale === 'ko' ? 'AI 변경' : 'Change AI' });
    await expect(picker).toBeVisible();
    await picker.selectOption('claude');
    await picker.focus();
    await expect(picker).toBeFocused();
    await picker.selectOption('codex');
    await expect(picker).toHaveValue('codex');
    await picker.selectOption('claude');
    const composer = page.locator('.ai-panel textarea');
    const bounds = await composer.boundingBox();
    expect(bounds.y).toBeGreaterThanOrEqual(0);
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(600);
    await composer.fill('Explain this article');
    await composer.press('Enter');
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
      )
      .toBe(1);
    const call = await page.evaluate(
      () => window.aiCalls.find((c) => c.cmd === 'start_ai_message').args,
    );
    expect(call.provider).toBe('claude');
    await expect(picker).toBeDisabled();
    await page.evaluate(() =>
      window.aiEmit({
        requestId: 'run-1',
        sessionId: 1,
        kind: 'done',
        text: 'An answer from the selected AI.',
      }),
    );
    await expect(picker).toBeEnabled();
    await page.screenshot({ path: `${output}/1100-600-${locale}-chat.png` });
    await page.goto(`${baseURL}/settings?section=ai`);
    const defaultPicker = page.getByRole('combobox', {
      name: locale === 'ko' ? '새 대화에 사용할 AI' : 'AI for new conversations',
    });
    await expect(defaultPicker).toBeVisible();
    await expect(defaultPicker).toHaveValue('claude');
    await defaultPicker.selectOption('codex');
    expect(await page.evaluate(() => localStorage.getItem('aequo:aiProvider'))).toBe('codex');
    await page.waitForTimeout(260);
    await page.screenshot({ path: `${output}/1100-600-${locale}-settings.png` });
    await page.goto(baseURL);
    await page.locator('.post').first().click();
    await expect(
      page.getByRole('combobox', { name: locale === 'ko' ? 'AI 변경' : 'Change AI' }),
    ).toHaveValue('codex');
    expect(errors).toEqual([]);
    results.push({
      scenario:
        'Minimum height, native AI selection and focus, selected provider dispatch and default persistence',
      locale,
      passed: true,
    });
    await page.close();
  }
  // Visual regression fixture for citations, the conversation list and the integrated composer.
  for (const width of [1100, 1360]) {
    const page = await browser.newPage({ viewport: { width, height: 850 } });
    await fixture(page, 'ko');
    await page.goto(baseURL);
    await page.locator('.post').first().click();
    const composer = page.locator('.ai-panel textarea');
    const prompt =
      '보안 행사와 업계 동향: 웨비나·조찬세미나·골프 마스터즈 일정과 핵심 내용을 정리해줘';
    const answer =
      '**요지:** 웨비나와 정례 조찬세미나, 업계 교류 행사로 구성된 일정입니다.[1]\n\n- **IT·정보보안 웨비나:** 최신 기술과 대응 사례를 소개하는 온라인 행사입니다.[1]\n- **CISO 조찬세미나:** 보안 책임자들이 위협과 정책 동향을 나누는 자리입니다.[1]\n- **골프 마스터즈:** 업계 리더의 교류를 위한 행사입니다.[1]\n\n행사별 일정과 참여 대상은 참고 글에서 확인할 수 있습니다.[1]';
    await composer.fill(prompt);
    await composer.press('Enter');
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
      )
      .toBe(1);
    await page.evaluate(
      (answer) => window.aiEmit({ requestId: 'run-1', sessionId: 1, kind: 'done', text: answer }),
      answer,
    );
    const badges = page.locator('.citation');
    await expect(badges).toHaveCount(5);
    await expect(page.locator('.answer-source')).toHaveCount(1);
    await expect(badges.first()).toHaveAttribute('title', /보안 행사/);
    expect(await badges.first().evaluate((el) => getComputedStyle(el).whiteSpace)).toBe('nowrap');
    await page.mouse.move(0, 0);
    await page.waitForTimeout(260);
    await page.locator('.ai-panel').screenshot({ path: `${output}/${width}-citations.png` });
    await page.getByRole('combobox', { name: 'AI 변경' }).selectOption('claude');
    await composer.fill(prompt);
    await composer.press('Enter');
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
      )
      .toBe(2);
    await page.evaluate(
      (answer) => window.aiEmit({ requestId: 'run-2', sessionId: 2, kind: 'done', text: answer }),
      answer,
    );
    await page.getByRole('button', { name: '이전 대화', exact: true }).click();
    const history = page.getByRole('dialog', { name: '이전 대화' });
    await expect(history.getByRole('radiogroup', { name: '대화 범위' })).toBeVisible();
    await expect(history.locator('li')).toHaveCount(2);
    await expect(history.locator('[aria-current="true"]')).toHaveCount(1);
    await page.mouse.move(0, 0);
    await page.waitForTimeout(260);
    await page.locator('.ai-panel').screenshot({ path: `${output}/${width}-history.png` });
    await page.getByRole('button', { name: '이전 대화', exact: true }).click();
    if (width === 1100) await page.keyboard.press('Escape');
    await page.locator('[data-post-id="2"]').click();
    if (width === 1100)
      await page.getByRole('button', { name: /이 글에 대해 AI에게 묻기/ }).click();
    const surface = page.locator('.composer-surface');
    await expect(surface.locator('.suggestion')).toBeVisible();
    await expect(
      surface.getByRole('button', { name: '이 대화에 추가', exact: true }),
    ).toBeVisible();
    await page.mouse.move(0, 0);
    await page.waitForTimeout(260);
    await page.locator('.ai-panel').screenshot({ path: `${output}/${width}-suggestion.png` });
    const bounds = await surface.boundingBox();
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
    await surface.getByRole('button', { name: '이 대화에 추가', exact: true }).click();
    await expect(surface.locator('.suggestion')).toBeHidden();
    results.push({
      scenario: 'Citation badges and sources, neutral history filter and integrated composer',
      width,
      passed: true,
    });
    await page.close();
  }
  // Open AI without a reader; references are explicitly chosen with @.
  for (const [locale, width, height] of [
    ['ko', 1100, 600],
    ['en', 1360, 850],
  ]) {
    const page = await browser.newPage({ viewport: { width, height } });
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await fixture(page, locale);
    await page.goto(baseURL);
    const openAI = page.getByRole('button', {
      name: locale === 'ko' ? 'AI 대화' : 'AI conversation',
      exact: true,
    });
    await openAI.click();
    await expect(page.locator('.ai-pane')).toBeVisible();
    await expect(page.locator('.reader-pane')).toHaveCount(0);
    const composer = page.locator('.ai-panel textarea');
    await composer.fill(locale === 'ko' ? 'Summarize @보안' : 'Summarize @Article');
    const options = page.locator('.mention-picker [role="option"]');
    await expect(options).toHaveCount(25);
    await composer.press('ArrowDown');
    await composer.press('ArrowDown');
    await composer.press('ArrowDown');
    await composer.press('ArrowDown');
    await page.mouse.move(0, 0);
    await expect.poll(async () => (await composer.boundingBox()).height).toBeLessThanOrEqual(50);
    await page.screenshot({ path: `${output}/${width}-main-overview.png` });
    await page.locator('.ai-panel').screenshot({ path: `${output}/${width}-main-mentions.png` });
    await composer.press('Enter');
    await expect(composer).toHaveValue('Summarize ');
    expect(
      await page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
    ).toBe(0);
    await composer.press('Enter');
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
      )
      .toBe(1);
    const first = await page.evaluate(
      () => window.aiCalls.find((c) => c.cmd === 'start_ai_message').args,
    );
    expect(first.contextItemIds).toEqual([5]);
    expect(first.prompt).toBe('Summarize');
    await page.evaluate(() =>
      window.aiEmit({
        requestId: 'run-1',
        sessionId: 1,
        kind: 'done',
        text: 'An answer about the selected article [1].',
      }),
    );
    await page.locator('.citation').click();
    await expect(page.locator('.reader h1')).toContainText(
      locale === 'ko' ? '내용 5' : 'Article 5:',
    );
    if (width === 1100) await page.keyboard.press('Escape');
    await page.locator('[data-post-id="6"]').click();
    if (width === 1100) await openAI.click();
    await expect(page.locator('.ai-pane')).toBeVisible();
    await expect(page.locator('.suggestion')).toHaveCount(0);
    await composer.fill('Compare @this');
    await expect(page.getByRole('option', { name: /@this/ })).toBeVisible();
    await composer.press('Enter');
    await expect(composer).toHaveValue('Compare ');
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'add_ai_context_items').length),
      )
      .toBe(1);
    await composer.press('Enter');
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
      )
      .toBe(2);
    const second = await page.evaluate(
      () => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').at(-1).args,
    );
    expect(second.contextItemIds).toEqual([5, 6]);
    expect(second.sessionId).toBe(1);
    await page.evaluate(() =>
      window.aiEmit({
        requestId: 'run-1',
        sessionId: 1,
        kind: 'done',
        text: 'A comparison of both references [1] [2].',
      }),
    );
    await composer.fill('@nomatchingarticle');
    await expect(
      page.getByText(
        locale === 'ko'
          ? '일치하는 글이 없어요. 다른 제목이나 검색어로 찾아보세요.'
          : 'No matching articles. Try another title or keyword.',
        { exact: true },
      ),
    ).toBeVisible();
    await composer.press('Escape');
    await expect(page.locator('.mention-picker')).toHaveCount(0);
    await expect(page.locator('.ai-pane')).toBeVisible();
    await composer.fill('');
    await page.locator('.composer-surface .context-trigger').click();
    const references = page.getByRole('dialog', {
      name: locale === 'ko' ? '참고 글 확인 및 관리' : 'View and manage reference articles',
    });
    await expect(references).toBeVisible();
    const box = await references.boundingBox();
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.y + box.height).toBeLessThanOrEqual(height);
    await references.locator('.remove').first().click();
    await expect
      .poll(() =>
        page.evaluate(
          () => window.aiCalls.filter((c) => c.cmd === 'remove_ai_context_item').length,
        ),
      )
      .toBe(1);
    await page.locator('.composer-surface .context-trigger').press('Escape');
    await composer.fill('One reference remains');
    await composer.press('Enter');
    await expect
      .poll(() =>
        page.evaluate(() => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').length),
      )
      .toBe(3);
    const third = await page.evaluate(
      () => window.aiCalls.filter((c) => c.cmd === 'start_ai_message').at(-1).args,
    );
    expect(third.contextItemIds).toEqual([6]);
    await page.evaluate(() =>
      window.aiEmit({
        requestId: 'run-1',
        sessionId: 1,
        kind: 'done',
        text: 'Only the remaining reference is used.',
      }),
    );
    await page.mouse.move(0, 0);
    await page
      .locator('.ai-panel')
      .screenshot({ path: `${output}/${width}-main-conversation.png` });
    expect(await page.evaluate(() => localStorage.getItem('aequo:aiStandalone'))).toBe('true');
    await page.reload();
    await expect(page.locator('.ai-pane')).toBeVisible();
    expect(errors).toEqual([]);
    results.push({
      scenario:
        'Main-screen AI, @ article search, @this, stable conversation, source removal and restored panel',
      locale,
      width,
      passed: true,
    });
    await page.close();
  }
  await writeFile(`${output}/results.json`, JSON.stringify(results, null, 2) + '\n');
  console.log(`Passed ${results.length} AI context browser scenarios. Results: ${output}`);
} catch (error) {
  const page = browser
    .contexts()
    .flatMap((context) => context.pages())
    .at(-1);
  await page?.screenshot({ path: `${output}/failure.png` }).catch(() => {});
  await writeFile(`${output}/failure.txt`, String(error));
  throw error;
} finally {
  await browser.close();
}
