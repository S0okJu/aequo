<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { fade } from 'svelte/transition';
  import { goto } from '$app/navigation';
  import { resolve } from '$app/paths';
  import { page } from '$app/state';
  import AddFeedDialog from '$lib/components/AddFeedDialog.svelte';
  import SettingsAi from '$lib/components/settings/SettingsAi.svelte';
  import SettingsGeneral from '$lib/components/settings/SettingsGeneral.svelte';
  import SettingsStorage from '$lib/components/settings/SettingsStorage.svelte';
  import SettingsPerformance from '$lib/components/settings/SettingsPerformance.svelte';
  import SettingsSubscriptions from '$lib/components/settings/SettingsSubscriptions.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import StatePlaceholder from '$lib/components/StatePlaceholder.svelte';
  import type { IconName } from '$lib/icons';
  import { motionDuration } from '$lib/motion';
  import { delayed } from '$lib/pending.svelte';
  import { listFeedFolders, listFeeds } from '$lib/api';
  import { showToast } from '$lib/toast.svelte.ts';
  import { t, type MessageKey } from '$lib/i18n/index.svelte';
  import type { FeedFolder, FeedSummary } from '$lib/types';

  type Section = 'general' | 'subscriptions' | 'ai' | 'performance' | 'storage';
  const SECTIONS: Section[] = ['general', 'ai', 'performance', 'storage', 'subscriptions'];
  const PREFERENCE_SECTIONS = SECTIONS.filter((item) => item !== 'subscriptions');
  const SECTION_ICONS: Record<Section, IconName> = {
    general: 'settings',
    subscriptions: 'inbox',
    ai: 'ask-ai',
    storage: 'database',
    performance: 'monitor',
  };
  // Links from before the sectioned layout (`?tab=`) still land somewhere sensible.
  const LEGACY_TABS: Record<string, Section> = {
    feeds: 'subscriptions',
    folders: 'subscriptions',
    storage: 'storage',
    appearance: 'general',
  };

  // The section lives in the URL (`?section=`), so direct links and
  // back/forward work (docs/UI_SIMPLIFICATION_PLAN.md §3.10).
  const section = $derived.by((): Section => {
    const params = page.url.searchParams;
    const requested = params.get('section') ?? '';
    if ((SECTIONS as string[]).includes(requested)) return requested as Section;
    return LEGACY_TABS[params.get('tab') ?? ''] ?? 'general';
  });

  let feeds = $state<FeedSummary[]>([]);
  let folders = $state<FeedFolder[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let addFeedOpen = $state(false);
  const hasData = () => feeds.length > 0 || folders.length > 0;
  // First load replaces the whole area with a loading/error state; once
  // there's data, a later refresh failure stays a small inline note instead
  // of yanking away a screen the user can already see and use (§6.3 "갱신
  // 중 화면이 비거나 깜빡이지 않는다").
  const showFirstLoad = delayed(() => loading && !hasData());
  let selectedFeedId = $state<number | null>(null);
  let heading = $state<HTMLHeadingElement | undefined>(undefined);
  let content = $state<HTMLElement | undefined>(undefined);

  function jumpToSection(target: Section, focus = false) {
    const element = content?.querySelector<HTMLElement>(`[data-settings-heading="${target}"]`);
    content?.scrollTo({ top: 0, behavior: 'instant' });
    if (focus) (element ?? heading)?.focus({ preventScroll: true });
    rememberPosition();
  }

  let paneWidth = 0;
  let paneHeight = 0;
  let scrollAnchor: { element: HTMLElement; top: number } | null = null;

  function rememberPosition() {
    if (!content || section === 'subscriptions') return;
    if (paneWidth && (content.clientWidth !== paneWidth || content.clientHeight !== paneHeight))
      return;
    const focused = document.activeElement?.closest<HTMLElement>('.settings-row');
    const top = content.getBoundingClientRect().top;
    const row =
      focused && content.contains(focused)
        ? focused
        : Array.from(content.querySelectorAll<HTMLElement>('.settings-row')).find(
            (element) =>
              element.offsetHeight > 0 && element.getBoundingClientRect().bottom > top + 16,
          );
    scrollAnchor = row ? { element: row, top: row.getBoundingClientRect().top } : null;
  }

  $effect(() => {
    const target = section;
    const pane = content;
    if (!pane) return;
    let frame = 0;
    let disposed = false;
    void tick().then(() => {
      if (disposed) return;
      frame = requestAnimationFrame(() => {
        jumpToSection(target);
        rememberPosition();
      });
    });
    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
    };
  });

  $effect(() => {
    const pane = content;
    if (!pane || typeof ResizeObserver === 'undefined') return;
    paneWidth = pane.clientWidth;
    paneHeight = pane.clientHeight;
    const observer = new ResizeObserver(() => {
      if (paneWidth === pane.clientWidth && paneHeight === pane.clientHeight) return;
      paneWidth = pane.clientWidth;
      paneHeight = pane.clientHeight;
      if (section !== 'subscriptions' && scrollAnchor?.element.isConnected) {
        pane.scrollTop += scrollAnchor.element.getBoundingClientRect().top - scrollAnchor.top;
      }
      rememberPosition();
    });
    observer.observe(pane);
    return () => observer.disconnect();
  });

  // One selection highlight that slides between menu items instead of each
  // item painting its own (docs/SIDEBAR_SETTINGS_UX_GOAL.md §4).
  let navList = $state<HTMLUListElement | undefined>(undefined);
  let navLinks = $state<HTMLAnchorElement[]>([]);
  let indicator = $state<{ top: number; height: number } | null>(null);
  let indicatorReady = $state(false);

  function measureIndicator() {
    const link = navLinks[SECTIONS.indexOf(section)];
    indicator = link ? { top: link.offsetTop, height: link.offsetHeight } : null;
  }

  $effect(() => {
    void section;
    measureIndicator();
  });

  $effect(() => {
    if (!navList) return;
    const observer =
      typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measureIndicator);
    observer?.observe(navList);
    const frame = requestAnimationFrame(() => (indicatorReady = true));
    return () => {
      observer?.disconnect();
      cancelAnimationFrame(frame);
    };
  });

  async function refresh() {
    loading = true;
    try {
      [feeds, folders] = await Promise.all([listFeeds(), listFeedFolders()]);
      loadError = null;
    } catch (error) {
      loadError = String(error);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    const feedParam = Number(page.url.searchParams.get('feed'));
    if (Number.isInteger(feedParam) && feedParam > 0) selectedFeedId = feedParam;
    refresh();
  });

  function sectionHref(target: Section): string {
    return resolve(`/settings?section=${target}`);
  }

  async function openSection(event: MouseEvent, target: Section) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0)
      return;
    event.preventDefault();
    if (target !== section) {
      await goto(resolve(`/settings?section=${target}`), { keepFocus: true, noScroll: true });
    }
    await tick();
    // Moving focus to the new section's heading tells screen readers where
    // they landed; the heading isn't otherwise in the tab order.
    jumpToSection(target, true);
  }

  function back() {
    goto(resolve('/'));
  }

  async function handleFeedAdded(feedId: number) {
    addFeedOpen = false;
    await refresh();
    selectedFeedId = feedId;
    showToast(t('toast.feedAdded'), { type: 'success' });
  }

  function handleKeydown(event: KeyboardEvent) {
    // Sections handle their own Escape first (inline editors, the detail
    // panel); anything left over leaves settings.
    if (event.key !== 'Escape' || event.defaultPrevented || addFeedOpen) return;
    const active = document.activeElement;
    if (active && ['INPUT', 'TEXTAREA', 'SELECT'].includes(active.tagName)) return;
    back();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="settings-layout">
  <nav class="settings-nav" aria-label={t('settings.nav')}>
    <p class="nav-title">{t('settings.title')}</p>
    <button type="button" class="back ui-action" onclick={back}>
      <Icon name="chevron-left" size={14} /><span>{t('settings.back')}</span><kbd>Esc</kbd>
    </button>
    <ul bind:this={navList} class:ready={indicatorReady}>
      {#if indicator}
        <li
          class="indicator"
          aria-hidden="true"
          style:height="{indicator.height}px"
          style:transform="translateY({indicator.top}px)"
        ></li>
      {/if}
      {#each SECTIONS as item, index (item)}
        <li>
          <!-- eslint-disable svelte/no-navigation-without-resolve -->
          <a
            bind:this={navLinks[index]}
            href={sectionHref(item)}
            aria-current={section === item ? 'page' : undefined}
            onclick={(event) => openSection(event, item)}
            ><Icon name={SECTION_ICONS[item]} size={15} />{t(
              `settings.section.${item}` as MessageKey,
            )}</a
          >
          <!-- eslint-enable svelte/no-navigation-without-resolve -->
        </li>
      {/each}
    </ul>
  </nav>

  <main
    onscroll={rememberPosition}
    onwheel={() => (scrollAnchor = null)}
    ontouchstart={() => (scrollAnchor = null)}
    onfocusin={rememberPosition}
    bind:this={content}
    class="settings-content"
    class:wide={section === 'subscriptions'}
  >
    {#if section === 'subscriptions' && loadError && hasData()}
      <!-- A refresh after an earlier success failed - the section below
           still has data to show, so this stays a small note rather than
           replacing the screen (unlike the first-load StatePlaceholder).
           Global/unrelated to which section is open, so it sits outside
           the section-keyed block below and never re-fades on a switch. -->
      <p class="load-error" role="alert">{t('settings.loadError', { error: loadError })}</p>
    {/if}
    {#if section === 'subscriptions'}
      <div class="section-fade" in:fade={{ duration: motionDuration(120) }}>
        <header class="section-header">
          <h1 bind:this={heading} tabindex="-1">{t('settings.section.subscriptions')}</h1>
          <p class="section-desc">{t('settings.section.subscriptions.desc')}</p>
        </header>
        {#if loading && !hasData()}
          {#if showFirstLoad.value}<StatePlaceholder kind="loading" />{/if}
        {:else if loadError && !hasData()}
          <StatePlaceholder kind="error" title={t('state.error.loadFailed.title')} body={loadError}>
            {#snippet action()}
              <button type="button" class="ui-action ui-action--secondary" onclick={refresh}>
                {t('state.error.loadFailed.action')}
              </button>
            {/snippet}
          </StatePlaceholder>
        {:else}
          <div class="section-body">
            <SettingsSubscriptions
              {feeds}
              {folders}
              bind:selectedId={selectedFeedId}
              onChanged={refresh}
              onAddFeed={() => (addFeedOpen = true)}
            />
          </div>
        {/if}
      </div>
    {:else}
      <div class="section-fade preferences" in:fade={{ duration: motionDuration(120) }}>
        {#each PREFERENCE_SECTIONS as item (item)}
          <section
            class="preference-section"
            hidden={section !== item}
            aria-labelledby="settings-heading-{item}"
          >
            <header class="section-header">
              <h1 id="settings-heading-{item}" data-settings-heading={item} tabindex="-1">
                {t(`settings.section.${item}` as MessageKey)}
              </h1>
              <p class="section-desc">{t(`settings.section.${item}.desc` as MessageKey)}</p>
            </header>
            <div class="section-body">
              {#if item === 'general'}<SettingsGeneral />
              {:else if item === 'ai'}<SettingsAi />
              {:else if item === 'performance'}<SettingsPerformance />
              {:else}<SettingsStorage onCleaned={refresh} />{/if}
            </div>
          </section>
        {/each}
      </div>
    {/if}
  </main>
</div>

<AddFeedDialog
  open={addFeedOpen}
  {folders}
  onAdded={handleFeedAdded}
  onClose={() => (addFeedOpen = false)}
/>

<style>
  .settings-layout {
    display: grid;
    height: 100vh;
    height: 100dvh;
    grid-template-columns: 184px minmax(0, 1fr);
    background: var(--color-bg);
  }

  .settings-nav {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    overflow-y: auto;
    padding: var(--space-4) var(--space-2);
    border-right: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
  }

  .nav-title {
    margin: 0 var(--space-2) var(--space-2);
    color: var(--color-text);
    font-size: var(--text-heading);
    font-weight: var(--weight-bold);
  }

  .back {
    display: inline-flex;
    align-self: stretch;
    align-items: center;
    gap: var(--space-1);
    margin-bottom: var(--space-2);
    padding: 4px var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  .back:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  .back:hover :global(svg) {
    transform: translateX(-2px);
  }

  .back :global(svg) {
    transition: transform var(--transition);
  }

  kbd {
    margin-left: auto;
    padding: 0 5px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-xs);
    color: var(--color-text-tertiary);
    font: inherit;
    font-size: var(--text-caption);
  }

  ul {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .indicator {
    position: absolute;
    top: 0;
    right: 0;
    left: 0;
    border-radius: var(--radius-sm);
    background: var(--color-selection-bg);
    pointer-events: none;
  }

  ul.ready .indicator {
    transition:
      transform var(--transition-layout),
      height var(--transition-layout);
  }

  ul a {
    position: relative;
    display: flex;
    min-height: 32px;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-3);
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    font-size: var(--text-body);
    text-decoration: none;
    transition:
      background-color var(--transition),
      color var(--transition);
  }

  ul a:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  /* The sliding indicator paints the selection; hover must not cover it. */
  ul a[aria-current='page'] {
    background: transparent;
    color: var(--color-text);
    font-weight: var(--weight-medium);
  }

  .settings-content {
    display: flex;
    min-height: 0;
    flex-direction: column;
    min-width: 0;
    overflow-y: auto;
    overscroll-behavior-y: contain;
    scrollbar-gutter: stable;
    width: 100%;
  }

  /* The subscriptions list + detail needs the room and scrolls internally. */
  .settings-content.wide {
    overflow: hidden;
    scrollbar-gutter: auto;
  }

  /* Width belongs to the content, while scrolling belongs to the full pane. */
  .section-fade {
    display: flex;
    flex: 0 0 auto;
    flex-direction: column;
    width: 100%;
    padding: var(--space-4) var(--space-5);
    --settings-content-width: 1120px;
  }

  .preferences {
    gap: var(--space-5);
  }

  .preference-section {
    min-width: 0;
    width: 100%;
    max-width: var(--settings-content-width);
  }

  [data-settings-heading] {
    scroll-margin-top: var(--space-4);
  }

  .wide .section-fade {
    min-height: 0;
    flex: 1;
    max-width: none;
    padding-right: 0;
    padding-bottom: 0;
  }

  .wide .section-header {
    margin-right: var(--space-5);
  }

  .section-header {
    flex-shrink: 0;
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  h1 {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: var(--weight-bold);
  }

  .section-desc {
    margin: var(--space-1) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-body);
  }

  .section-body {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    padding-top: var(--space-4);
  }

  .wide .section-body {
    overflow: hidden;
  }

  h1:focus {
    outline: none;
  }

  .load-error {
    color: var(--color-error);
    font-size: var(--text-small);
  }
</style>
