<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { nextArticleRevision } from '$lib/articleRequest';
  import {
    cancelArticleRead,
    createAnnotation,
    deleteAnnotation,
    listItemAnnotations,
    loadArticle,
    updateAnnotation,
  } from '$lib/api';
  import FeedContent from './FeedContent.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import IconButton from './IconButton.svelte';
  import ErrorCard from './ErrorCard.svelte';
  import Spinner from './Spinner.svelte';
  import {
    feedDisplayName,
    articleDateTitle,
    articleDateLabel,
    stripSyndicationFooter,
  } from '$lib/format';
  import { t, type MessageKey } from '$lib/i18n/index.svelte';
  import { delayed } from '$lib/pending.svelte';
  import { clickOutside } from '$lib/clickOutside';
  import type { Annotation, ArticleView, FeedSummary, Item } from '$lib/types';
  import type { CreateAnnotationInput, ReanchoredUpdate } from './FeedContent.svelte';

  interface Props {
    item: Item | null;
    feeds: FeedSummary[];
    onClose: () => void;
    onToggleSaved: (itemId: number) => void;
    onAskAi?: (quotedText: string) => void;
    /** Whether the AI panel is showing beside this post. */
    aiOpen?: boolean;
    /** This post has saved conversations, so the button gets a quiet dot. */
    aiHistory?: boolean;
    onToggleAi?: () => void;
    onSummarize?: () => void;
    aiBusy?: boolean;
    onMarkUnread?: (itemId: number) => void;
    initialScrollTop?: number;
    onScrollPositionChange?: (top: number) => void;
  }

  let {
    item,
    feeds,
    onClose,
    onToggleSaved,
    onAskAi,
    aiOpen = false,
    aiHistory = false,
    onToggleAi,
    onSummarize,
    aiBusy = false,
    onMarkUnread,
    initialScrollTop = 0,
    onScrollPositionChange,
  }: Props = $props();
  let article = $state<ArticleView | null>(null);
  let loadingArticle = $state(false);
  let refreshingArticle = $state(false);
  let articleCommandError = $state<string | null>(null);
  let annotations = $state<Annotation[]>([]);
  let orphanedAnnotationIds = $state<number[]>([]);
  let contentEl = $state<HTMLDivElement | undefined>(undefined);

  let readerScrollEl = $state<HTMLDivElement>();
  let readerArticleEl = $state<HTMLElement>();
  let annotationsSectionEl = $state<HTMLElement>();
  let pendingScrollTop: number | null = null;
  let restoreObserver: ResizeObserver | null = null;
  const selectedPostId = $derived(item?.id);
  // Tagged with which operation failed so the ErrorCard title can say what
  // broke, instead of one generic "annotations unavailable" for every case
  // (docs/UNIFIED_UX_MOTION_PLAN.md §7.5 A11).
  let annotationError = $state<{
    action: 'load' | 'create' | 'update' | 'delete';
    message: string;
  } | null>(null);
  let readerGeneration = 0;
  let currentRequest: number | null = null;
  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  let flashedMark: Element | null = null;

  function isCurrent(generation: number, itemId: number): boolean {
    return generation === readerGeneration && item?.id === itemId;
  }

  function beginArticle(itemId: number, generation: number, forceRefresh: boolean) {
    const revision = nextArticleRevision();
    currentRequest = revision;
    loadingArticle = !forceRefresh;
    refreshingArticle = forceRefresh;
    articleCommandError = null;
    void loadArticle(itemId, forceRefresh, revision)
      .then((result) => {
        if (isCurrent(generation, itemId) && currentRequest === revision) article = result;
      })
      .catch((error) => {
        if (isCurrent(generation, itemId) && currentRequest === revision)
          articleCommandError = String(error);
      })
      .finally(() => {
        if (isCurrent(generation, itemId) && currentRequest === revision) {
          currentRequest = null;
          loadingArticle = false;
          refreshingArticle = false;
        }
      });
  }

  $effect(() => {
    // Read/save/title changes do not reload the same article or its annotations.
    const itemId = selectedPostId;
    return untrack(() => {
      const generation = ++readerGeneration;
      restoreObserver?.disconnect();
      pendingScrollTop = initialScrollTop > 0 ? initialScrollTop : null;
      if (readerScrollEl) readerScrollEl.scrollTop = 0;
      article = null;
      annotations = [];
      orphanedAnnotationIds = [];
      articleCommandError = null;
      annotationError = null;
      loadingArticle = false;
      refreshingArticle = false;
      overflowOpen = false;
      if (itemId != null) {
        beginArticle(itemId, generation, false);
        // Independent failures must not hide a successfully loaded article.
        void listItemAnnotations(itemId)
          .then((result) => {
            if (isCurrent(generation, itemId)) {
              annotations = [
                ...new Map([...result, ...annotations].map((entry) => [entry.id, entry])).values(),
              ];
            }
          })
          .catch((error) => {
            if (isCurrent(generation, itemId))
              annotationError = { action: 'load', message: String(error) };
          });
      }
      return () => {
        readerGeneration++;
        if (currentRequest !== null) {
          void cancelArticleRead(currentRequest).catch(() => undefined);
          currentRequest = null;
        }
        clearTimeout(flashTimer);
        flashedMark?.classList.remove('aequo-flash');
        flashedMark = null;
        // Release large payloads even if an uncancellable annotation reply is late.
        article = null;
        annotations = [];
      };
    });
  });

  function stopPositionRestore() {
    pendingScrollTop = null;
    restoreObserver?.disconnect();
  }

  $effect(() => {
    const element = readerScrollEl;
    if (!element) return;
    for (const event of ['wheel', 'pointerdown', 'keydown'])
      element.addEventListener(event, stopPositionRestore, { passive: true });
    return () => {
      for (const event of ['wheel', 'pointerdown', 'keydown'])
        element.removeEventListener(event, stopPositionRestore);
    };
  });

  $effect(() => {
    const loaded = article;
    const root = readerArticleEl;
    const element = readerScrollEl;
    if (!loaded || !root || !element) return;
    return untrack(() => {
      if (pendingScrollTop === null) return;
      const generation = readerGeneration;
      let stopped = false;
      const restore = () => {
        void tick().then(() => {
          if (stopped || generation !== readerGeneration || pendingScrollTop === null) return;
          element.scrollTop = pendingScrollTop;
          onScrollPositionChange?.(element.scrollTop);
        });
      };
      const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(restore);
      restoreObserver = observer;
      observer?.observe(root);
      restore();
      return () => {
        stopped = true;
        observer?.disconnect();
        if (restoreObserver === observer) restoreObserver = null;
      };
    });
  });

  function handleCreateAnnotation(input: CreateAnnotationInput) {
    if (!item) return;
    const itemId = item.id;
    const generation = readerGeneration;
    annotationError = null;
    createAnnotation({ itemId, ...input })
      .then((created) => {
        if (isCurrent(generation, itemId)) annotations = [...annotations, created];
      })
      .catch((error) => {
        if (isCurrent(generation, itemId))
          annotationError = { action: 'create', message: String(error) };
      });
  }

  function handleUpdateAnnotation(next: Annotation) {
    const itemId = item?.id;
    if (itemId == null || next.itemId !== itemId) return;
    const generation = readerGeneration;
    annotationError = null;
    updateAnnotation(next)
      .then((updated) => {
        if (isCurrent(generation, itemId))
          annotations = annotations.map((a) => (a.id === updated.id ? updated : a));
      })
      .catch((error) => {
        if (isCurrent(generation, itemId))
          annotationError = { action: 'update', message: String(error) };
      });
  }

  function handleDeleteAnnotation(id: number) {
    const itemId = item?.id;
    if (itemId == null) return;
    const generation = readerGeneration;
    annotationError = null;
    deleteAnnotation(id)
      .then(() => {
        if (isCurrent(generation, itemId)) annotations = annotations.filter((a) => a.id !== id);
      })
      .catch((error) => {
        if (isCurrent(generation, itemId))
          annotationError = { action: 'delete', message: String(error) };
      });
  }

  function handleReanchored(updates: ReanchoredUpdate[]) {
    for (const update of updates) {
      const existing = annotations.find((a) => a.id === update.id);
      if (!existing) continue;
      updateAnnotation({ ...existing, ...update }).catch(() => {
        // Best-effort: the in-memory offsets are already correct for this
        // render; a failed persist just means it's recomputed next time
        // this post's content hash is re-checked.
      });
    }
    annotations = annotations.map((a) => {
      const update = updates.find((u) => u.id === a.id);
      return update ? { ...a, ...update } : a;
    });
  }

  function scrollToAnnotation(id: number) {
    const mark = contentEl?.querySelector(`[data-annotation-id="${id}"]`);
    if (!mark) return;
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    mark.scrollIntoView({ behavior: reduceMotion ? 'instant' : 'smooth', block: 'center' });
    clearTimeout(flashTimer);
    flashedMark?.classList.remove('aequo-flash');
    flashedMark = mark;
    mark.classList.add('aequo-flash');
    flashTimer = setTimeout(() => {
      mark.classList.remove('aequo-flash');
      flashedMark = null;
    }, 1200);
  }

  function scrollToAnnotationsSection() {
    if (!annotationsSectionEl) return;
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    annotationsSectionEl.scrollIntoView({ behavior: reduceMotion ? 'instant' : 'smooth' });
  }

  // Highlights and memos together, sorted by position - a highlight with
  // no note previously had no index entry at all, and an orphaned highlight
  // (position lost on re-extraction) had nowhere to show up either
  // (docs/UNIFIED_UX_MOTION_PLAN.md §7.5 A6/A7).
  const annotationIndex = $derived([...annotations].sort((a, b) => a.startOffset - b.startOffset));

  const feed = $derived(feeds.find((entry) => entry.id === item?.feedId));

  const baseUrl = $derived(item?.link ?? feed?.url ?? '');
  const previewHtml = $derived(
    (item?.preview ?? '').replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;'),
  );
  const fullArticleLoaded = $derived(Boolean(article?.contentHtml || article?.rssContentHtml));
  const extractedArticleLoaded = $derived(Boolean(article?.contentHtml));
  // 200ms-gated so opening an article that extracts quickly never flashes
  // this card (docs/UNIFIED_UX_MOTION_PLAN.md §6.3).
  const showFullTextLoading = delayed(() => loadingArticle && !fullArticleLoaded);

  function refreshArticle() {
    if (!item?.link || refreshingArticle) return;
    beginArticle(item.id, readerGeneration, true);
  }

  // --- Narrow-panel overflow menu (docs/UNIFIED_UX_MOTION_PLAN.md §7.2 R9) -
  // save/mark-unread/open-original/refresh collapse into this below 420px,
  // reusing FeedList.svelte's menu keyboard/focus pattern rather than a
  // second implementation of the same thing.

  let overflowOpen = $state(false);
  let overflowTrigger = $state<HTMLElement>();

  function closeOverflow(restoreFocus = false) {
    overflowOpen = false;
    if (restoreFocus) overflowTrigger?.focus();
  }

  function runOverflowAction(action: () => void) {
    action();
    closeOverflow();
  }

  function focusFirstMenuItem(node: HTMLElement) {
    node.querySelector<HTMLElement>('[role="menuitem"]:not(:disabled)')?.focus();
  }

  function handleOverflowKeydown(event: KeyboardEvent) {
    const items = [
      ...(event.currentTarget as HTMLElement).querySelectorAll<HTMLElement>(
        '[role="menuitem"]:not(:disabled)',
      ),
    ];
    const index = items.indexOf(document.activeElement as HTMLElement);
    if (event.key === 'Escape') {
      event.preventDefault();
      closeOverflow(true);
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      items[(index + 1) % items.length]?.focus();
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      items[(index - 1 + items.length) % items.length]?.focus();
    } else if (event.key === 'Tab') {
      closeOverflow();
    }
  }
</script>

{#if item}
  <div class="reader-root">
    <!-- Fixed action row, outside the scrolling body (docs/UNIFIED_UX_MOTION_PLAN.md
         §5.2, §5.5 결정 2): save/close etc. stay reachable while scrolled into
         a long article, instead of scrolling away with the title. -->
    <div class="reader-action-bar">
      <!-- Collapses into the ⋯ menu below (CSS only, via @container on
           .reader-pane) - save/mark-unread/open-original/refresh are useful
           but not what a cramped panel has room for
           (docs/UNIFIED_UX_MOTION_PLAN.md §7.2 R9). -->
      <div class="reader-utility-actions">
        <div class="secondary-actions">
          <IconButton
            icon={item.isSaved ? 'star' : 'star-outline'}
            label={item.isSaved ? t('common.unsavePost') : t('common.savePost')}
            active={item.isSaved}
            titleOverride={item.isSaved ? t('common.saved') : t('common.save')}
            onclick={() => onToggleSaved(item.id)}
          />
          {#if onMarkUnread && item.isRead}
            <IconButton
              icon="dot"
              label={t('reader.markUnread')}
              onclick={() => onMarkUnread(item.id)}
            />
          {/if}
          {#if item.link}
            <IconButton icon="external-link" label={t('reader.openOriginal')} href={item.link} />
          {/if}
          {#if extractedArticleLoaded}
            <IconButton
              icon="retry"
              label={t('reader.refreshFullText')}
              titleOverride={article?.cached
                ? t('reader.refreshFullTextCached')
                : t('reader.refreshFullText')}
              spinning={refreshingArticle}
              disabled={refreshingArticle}
              onclick={refreshArticle}
            />
          {/if}
        </div>
        <div class="overflow-anchor">
          <IconButton
            icon="more"
            label={t('reader.moreActions')}
            ariaHaspopup="menu"
            ariaExpanded={overflowOpen}
            onclick={(event) => {
              overflowTrigger = event.currentTarget as HTMLElement;
              overflowOpen = !overflowOpen;
            }}
          />
          {#if overflowOpen}
            <div
              class="menu ui-popup"
              role="menu"
              tabindex="-1"
              aria-label={t('reader.moreActions')}
              use:focusFirstMenuItem
              use:clickOutside={() => closeOverflow()}
              onkeydown={handleOverflowKeydown}
            >
              <button
                type="button"
                role="menuitem"
                onclick={() => runOverflowAction(() => onToggleSaved(item.id))}
                ><Icon name={item.isSaved ? 'star' : 'star-outline'} size={14} />{item.isSaved
                  ? t('common.unsavePost')
                  : t('common.savePost')}</button
              >
              {#if onMarkUnread && item.isRead}
                <button
                  type="button"
                  role="menuitem"
                  onclick={() => runOverflowAction(() => onMarkUnread(item.id))}
                  ><Icon name="dot" size={14} />{t('reader.markUnread')}</button
                >
              {/if}
              {#if item.link}
                <!-- eslint-disable svelte/no-navigation-without-resolve -->
                <a
                  role="menuitem"
                  href={item.link}
                  target="_blank"
                  rel="noopener noreferrer"
                  onclick={() => closeOverflow()}
                  ><Icon name="external-link" size={14} />{t('reader.openOriginal')}</a
                >
                <!-- eslint-enable svelte/no-navigation-without-resolve -->
              {/if}
              {#if extractedArticleLoaded}
                <button
                  type="button"
                  role="menuitem"
                  disabled={refreshingArticle}
                  onclick={() => runOverflowAction(refreshArticle)}
                  ><Icon name="retry" size={14} />{refreshingArticle
                    ? t('reader.tryingAgain')
                    : t('reader.refreshFullText')}</button
                >
              {/if}
            </div>
          {/if}
        </div>
        <IconButton
          icon="memo"
          label={t('reader.annotationsTitle')}
          hasDot={annotationIndex.length > 0}
          disabled={annotationIndex.length === 0}
          onclick={scrollToAnnotationsSection}
        />
      </div>
      <div class="reader-ai-actions">
        {#if onToggleAi}
          {#if onSummarize}
            <button
              type="button"
              class="reader-ai-action ui-action"
              disabled={aiBusy}
              title={t('reader.summarizeHint')}
              onclick={onSummarize}>{t('ai.chip.summarize')}</button
            >
          {/if}
          <button
            type="button"
            class="reader-ai-toggle ui-action"
            class:has-dot={aiHistory && !aiOpen}
            class:active={aiOpen}
            aria-label={aiOpen
              ? t('reader.closeAi')
              : aiHistory
                ? t('reader.askAiWithHistory')
                : t('reader.askAi')}
            aria-pressed={aiOpen}
            title={aiOpen ? t('reader.closeAi') : t('reader.askAiShort')}
            onclick={onToggleAi}
            ><Icon name="ask-ai" size={14} /><span
              >{t(aiOpen ? 'reader.closeAiAction' : 'reader.askAiAction')}</span
            ></button
          >
        {/if}
        <IconButton
          icon="close"
          label={t('reader.closeArticle')}
          titleOverride={t('common.close')}
          onclick={onClose}
        />
      </div>
    </div>
    <div
      class="reader-scroll"
      bind:this={readerScrollEl}
      onscroll={() => onScrollPositionChange?.(readerScrollEl?.scrollTop ?? 0)}
    >
      <article class="reader" bind:this={readerArticleEl}>
        <header class="reader-header">
          <p class="byline">
            <FeedIcon {feed} />
            <span class="source">{feed ? feedDisplayName(feed) : t('common.unknownFeed')}</span>
            <span aria-hidden="true">·</span>
            <time
              datetime={item.publishedAtKind === 'date'
                ? new Date(item.publishedAt * 1000).toISOString().slice(0, 10)
                : new Date(item.publishedAt * 1000).toISOString()}
              title={articleDateTitle(item)}
            >
              {articleDateLabel(item)}
            </time>
          </p>
          <h1>{item.title ?? '(untitled)'}</h1>
        </header>
        {#if (article?.error || articleCommandError) && extractedArticleLoaded}
          <ErrorCard
            title={t('reader.savedFullTextTitle')}
            body={article?.error ?? articleCommandError ?? ''}
            onRetry={refreshArticle}
            retryLabel={t('reader.tryRefreshAgain')}
            retrying={refreshingArticle}
            retryingLabel={t('reader.tryingAgain')}
          />
        {:else if (article?.error || articleCommandError) && !extractedArticleLoaded}
          <ErrorCard
            title={t('reader.rssVersionTitle')}
            body={article?.error ?? articleCommandError ?? ''}
            onRetry={refreshArticle}
            retryLabel={t('reader.tryFullTextAgain')}
            retrying={refreshingArticle || loadingArticle}
            retryingLabel={t('reader.tryingAgain')}
          />
        {:else if showFullTextLoading.value}
          <div class="article-loading">
            <Spinner size={17} />
            <div>
              <strong>{t('reader.loadingFullTitle')}</strong>
              <p>{t('reader.loadingFullBody')}</p>
            </div>
          </div>
        {/if}
        {#if annotationError}
          <ErrorCard
            title={t('reader.annotationError.title', {
              action: t(`annotation.action.${annotationError.action}` as MessageKey),
            })}
            body={annotationError.message}
            onDismiss={() => (annotationError = null)}
          />
        {/if}
        <!-- The article keeps its own language (line breaking, hyphenation,
           screen reader voice), independent of the UI language. -->
        {#key item.id}
          <div bind:this={contentEl} lang={feed?.language ?? undefined}>
            <FeedContent
              content={article?.contentHtml ??
                stripSyndicationFooter(article?.rssContentHtml ?? previewHtml)}
              baseUrl={article?.finalUrl ?? baseUrl}
              {annotations}
              onCreateAnnotation={handleCreateAnnotation}
              onUpdateAnnotation={handleUpdateAnnotation}
              onDeleteAnnotation={handleDeleteAnnotation}
              onReanchored={handleReanchored}
              onOrphanedChange={(ids) => (orphanedAnnotationIds = ids)}
              {onAskAi}
            />
          </div>
        {/key}
        {#if annotationIndex.length > 0}
          <section class="memo-list" bind:this={annotationsSectionEl}>
            <h2>{t('reader.annotationsTitle')}</h2>
            <ul>
              {#each annotationIndex as entry (entry.id)}
                <li>
                  <button
                    type="button"
                    class="memo-list-item"
                    class:orphaned={orphanedAnnotationIds.includes(entry.id)}
                    onclick={() => scrollToAnnotation(entry.id)}
                  >
                    <span class="memo-quote-row">
                      {#if entry.kind === 'highlight'}
                        <span
                          class="memo-swatch"
                          style:background="var(--hl-{entry.color ?? 'yellow'})"
                          aria-hidden="true"
                        ></span>
                      {/if}
                      <span class="memo-quote"
                        >“{entry.quotedText.slice(0, 80)}{entry.quotedText.length > 80
                          ? '…'
                          : ''}”</span
                      >
                    </span>
                    {#if entry.kind === 'memo'}
                      <span class="memo-note">{entry.note}</span>
                    {/if}
                    {#if orphanedAnnotationIds.includes(entry.id)}
                      <span class="memo-orphaned-badge">{t('reader.positionNotFound')}</span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </article>
    </div>
  </div>
{:else}
  <div class="empty-placeholder">
    <span class="empty-placeholder-icon"><Icon name="empty" size={20} /></span>
    <p>{t('reader.selectPost')}</p>
  </div>
{/if}

<style>
  .reader-root {
    display: flex;
    height: 100%;
    flex-direction: column;
    min-height: 0;
  }

  /* Fixed row: save/original/refresh/AI/close stay reachable while the
     title and byline below scroll out of view in a long article
     (docs/UNIFIED_UX_MOTION_PLAN.md §5.2). */
  .reader-action-bar {
    display: flex;
    min-height: 48px;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    flex-wrap: wrap;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  .reader-utility-actions,
  .reader-ai-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }
  .reader-ai-actions {
    margin-left: auto;
  }

  .reader-ai-toggle {
    color: var(--color-text);
  }
  .reader-ai-toggle.active {
    background: var(--color-selection-bg);
  }
  .reader-ai-toggle.has-dot::after {
    /* Decorative dot dimension, mirrors IconButton's own dot - left raw. */
    width: 5px;
    height: 5px;
    border-radius: var(--radius-pill);
    background: var(--color-accent);
    content: '';
  }
  .reader-ai-action:hover:not(:disabled),
  .reader-ai-toggle:hover {
    background: var(--color-hover-bg);
  }
  .reader-ai-action:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .reader-ai-action:focus-visible,
  .reader-ai-toggle:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  .secondary-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .overflow-anchor {
    position: relative;
    display: none;
  }

  /* Below this, save/mark-unread/open-original/refresh move into the ⋯
     menu instead of crowding out the title and the AI/close buttons
     (docs/UNIFIED_UX_MOTION_PLAN.md §7.2 R9). Keyed off .reader-pane's
     own container-type: inline-size (+page.svelte), not the window. */
  @container (max-width: 420px) {
    .secondary-actions {
      display: none;
    }

    .overflow-anchor {
      display: flex;
    }
  }

  .menu {
    position: absolute;
    z-index: 30;
    top: calc(100% + var(--space-1));
    right: 0;
    display: flex;
    width: 210px;
    flex-direction: column;
  }

  .menu button,
  .menu a {
    min-height: var(--control-size);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    border: 0;
    border-radius: var(--control-radius);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-small);
    text-align: left;
    text-decoration: none;
    cursor: pointer;
  }

  .menu button :global(svg),
  .menu a :global(svg) {
    flex-shrink: 0;
    color: var(--color-text-secondary);
  }

  .menu button:hover:not(:disabled),
  .menu button:focus-visible,
  .menu a:hover,
  .menu a:focus-visible {
    background: var(--color-hover-bg);
    outline: none;
  }

  .menu button:disabled {
    color: var(--color-text-tertiary);
    cursor: default;
  }

  .reader-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .reader {
    /* Promotes the --content-max-width token (72ch) that was defined but
       never used anywhere - a comfortable reading line length instead of
       a hand-picked 860px (docs/UNIFIED_UX_MOTION_PLAN.md §7.2 R1/R2,
       §7.6 결정 1). */
    width: min(var(--content-max-width), 100%);
    margin-inline: auto;
    padding: clamp(20px, 5cqw, 40px);
  }

  .reader-header {
    margin-bottom: var(--space-5);
    padding-bottom: var(--space-4);
    border-bottom: 1px solid var(--color-border);
  }

  .byline {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-2);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    white-space: nowrap;
  }

  .source {
    overflow: hidden;
    color: var(--color-text-secondary);
    font-weight: var(--weight-medium);
    text-overflow: ellipsis;
  }

  /* The two fallback notices moved to ErrorCard (global .error-card
     classes, src/routes/+layout.svelte); only the compact loading card
     keeps its own look here, matching that same card shape. */
  .article-loading {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: 0 0 var(--space-4);
    padding: var(--space-3);
    border: 1px solid color-mix(in srgb, var(--color-accent) 22%, var(--color-border));
    border-radius: var(--radius-lg);
    background: var(--color-bg-secondary);
  }

  .article-loading strong {
    color: var(--color-text);
    font-size: var(--text-body);
  }

  .article-loading p {
    margin: var(--space-1) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.45;
  }

  h1 {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-display);
    font-weight: var(--weight-bold);
    letter-spacing: -0.02em;
    line-height: 1.3;
    word-break: keep-all;
  }

  .memo-list {
    margin-top: var(--space-5);
    padding-top: var(--space-5);
    border-top: 1px solid var(--color-border);
  }

  .memo-list h2 {
    margin: 0 0 var(--space-2);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    font-weight: var(--weight-bold);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .memo-list ul {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .memo-list-item {
    display: flex;
    width: 100%;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    text-align: left;
    cursor: pointer;
  }

  .memo-list-item:hover {
    border-color: var(--color-accent);
  }

  .memo-list-item.orphaned {
    border-color: var(--color-error);
  }

  .memo-quote-row {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: var(--space-2);
  }

  .memo-swatch {
    /* Decorative color-dot dimension, same family as AnnotationPopover's
       swatches - left raw. */
    width: 10px;
    height: 10px;
    flex-shrink: 0;
    border-radius: 50%;
  }

  .memo-quote {
    overflow: hidden;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .memo-note {
    color: var(--color-text);
    font-size: var(--text-body);
  }

  .memo-orphaned-badge {
    color: var(--color-error);
    font-size: var(--text-caption);
    font-weight: var(--weight-bold);
  }

  :global(.aequo-flash) {
    animation: aequo-flash var(--motion-emphasis) ease;
  }

  @keyframes aequo-flash {
    0%,
    100% {
      outline: 2px solid transparent;
    }
    25% {
      outline: 2px solid var(--color-accent);
    }
  }
</style>
