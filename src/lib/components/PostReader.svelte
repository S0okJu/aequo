<script lang="ts">
  import {
    createAnnotation,
    deleteAnnotation,
    listItemAnnotations,
    loadArticle,
    updateAnnotation,
  } from '$lib/api';
  import FeedContent from './FeedContent.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import Spinner from './Spinner.svelte';
  import { feedDisplayName, formatDateTime, stripSyndicationFooter } from '$lib/format';
  import { t } from '$lib/i18n/index.svelte';
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
    onToggleAi?: () => void;
    onMarkUnread?: (itemId: number) => void;
  }

  let {
    item,
    feeds,
    onClose,
    onToggleSaved,
    onAskAi,
    aiOpen = false,
    onToggleAi,
    onMarkUnread,
  }: Props = $props();
  let article = $state<ArticleView | null>(null);
  let loadingArticle = $state(false);
  let refreshingArticle = $state(false);
  let articleCommandError = $state<string | null>(null);
  let annotations = $state<Annotation[]>([]);
  let orphanedAnnotationIds = $state<number[]>([]);
  let contentEl = $state<HTMLDivElement | undefined>(undefined);

  $effect(() => {
    const currentItem = item;
    let cancelled = false;
    article = null;
    annotations = [];
    orphanedAnnotationIds = [];
    articleCommandError = null;
    if (!currentItem?.id) {
      loadingArticle = false;
      return;
    }
    loadingArticle = true;
    Promise.all([loadArticle(currentItem.id), listItemAnnotations(currentItem.id)])
      .then(([articleResult, annotationsResult]) => {
        if (!cancelled) {
          article = articleResult;
          annotations = annotationsResult;
        }
      })
      .catch((error) => {
        if (!cancelled) articleCommandError = String(error);
      })
      .finally(() => {
        if (!cancelled) loadingArticle = false;
      });
    return () => {
      cancelled = true;
    };
  });

  function handleCreateAnnotation(input: CreateAnnotationInput) {
    if (!item) return;
    createAnnotation({ itemId: item.id, ...input })
      .then((created) => {
        annotations = [...annotations, created];
      })
      .catch((error) => {
        articleCommandError = String(error);
      });
  }

  function handleUpdateAnnotation(next: Annotation) {
    updateAnnotation(next)
      .then((updated) => {
        annotations = annotations.map((a) => (a.id === updated.id ? updated : a));
      })
      .catch((error) => {
        articleCommandError = String(error);
      });
  }

  function handleDeleteAnnotation(id: number) {
    deleteAnnotation(id)
      .then(() => {
        annotations = annotations.filter((a) => a.id !== id);
      })
      .catch((error) => {
        articleCommandError = String(error);
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
    mark.classList.add('aequo-flash');
    setTimeout(() => mark.classList.remove('aequo-flash'), 1200);
  }

  const memos = $derived(annotations.filter((a) => a.kind === 'memo'));

  const feed = $derived(feeds.find((entry) => entry.id === item?.feedId));

  const baseUrl = $derived(item?.link ?? feed?.url ?? '');
  const fullArticleLoaded = $derived(Boolean(article?.contentHtml || article?.rssContentHtml));
  const extractedArticleLoaded = $derived(Boolean(article?.contentHtml));

  async function refreshArticle() {
    if (!item?.link || refreshingArticle) return;
    refreshingArticle = true;
    articleCommandError = null;
    try {
      article = await loadArticle(item.id, true);
    } catch (error) {
      articleCommandError = String(error);
    } finally {
      refreshingArticle = false;
    }
  }
</script>

{#if item}
  <div class="reader-scroll">
    <article class="reader">
      <header class="reader-header">
        <div class="header-bar">
          <p class="byline">
            <FeedIcon {feed} />
            <span class="source">{feed ? feedDisplayName(feed) : t('common.unknownFeed')}</span>
            <span aria-hidden="true">·</span>
            <time datetime={new Date(item.publishedAt * 1000).toISOString()}>
              {formatDateTime(item.publishedAt)}
            </time>
          </p>
          <div class="header-buttons">
            <button
              class="icon-button"
              class:active={item.isSaved}
              type="button"
              aria-label={item.isSaved ? t('common.unsavePost') : t('common.savePost')}
              aria-pressed={item.isSaved}
              title={item.isSaved ? t('common.saved') : t('common.save')}
              onclick={() => onToggleSaved(item.id)}
              ><Icon name={item.isSaved ? 'star' : 'star-outline'} size={16} /></button
            >
            {#if onMarkUnread && item.isRead}
              <button
                class="icon-button"
                type="button"
                aria-label={t('reader.markUnread')}
                title={t('reader.markUnread')}
                onclick={() => onMarkUnread(item.id)}><Icon name="dot" size={16} /></button
              >
            {/if}
            {#if item.link}
              <!-- eslint-disable svelte/no-navigation-without-resolve -->
              <a
                class="icon-button"
                href={item.link}
                target="_blank"
                rel="noopener noreferrer"
                aria-label={t('reader.openOriginal')}
                title={t('reader.openOriginal')}><Icon name="external-link" size={16} /></a
              >
              <!-- eslint-enable svelte/no-navigation-without-resolve -->
            {/if}
            {#if extractedArticleLoaded}
              <button
                class="icon-button"
                class:spinning={refreshingArticle}
                type="button"
                aria-label={t('reader.refreshFullText')}
                title={article?.cached
                  ? t('reader.refreshFullTextCached')
                  : t('reader.refreshFullText')}
                disabled={refreshingArticle}
                onclick={refreshArticle}><Icon name="retry" size={16} /></button
              >
            {/if}
            {#if onToggleAi}
              <button
                class="icon-button"
                class:active={aiOpen}
                type="button"
                aria-label={aiOpen ? t('reader.closeAi') : t('reader.askAi')}
                aria-pressed={aiOpen}
                title={aiOpen ? t('reader.closeAi') : t('reader.askAiShort')}
                onclick={onToggleAi}><Icon name="ask-ai" size={16} /></button
              >
            {/if}
            <button
              class="icon-button"
              type="button"
              aria-label={t('reader.closeArticle')}
              title={t('common.close')}
              onclick={onClose}><Icon name="close" size={16} /></button
            >
          </div>
        </div>
        <h1>{item.title ?? '(untitled)'}</h1>
      </header>
      {#if (article?.error || articleCommandError) && extractedArticleLoaded}
        <section class="fallback-notice refresh-warning" aria-live="polite">
          <span class="notice-icon"><Icon name="info" size={13} /></span>
          <div>
            <strong>{t('reader.savedFullTextTitle')}</strong>
            <p>{article?.error ?? articleCommandError}</p>
            <button type="button" disabled={refreshingArticle} onclick={refreshArticle}>
              {refreshingArticle ? t('reader.tryingAgain') : t('reader.tryRefreshAgain')}
            </button>
          </div>
        </section>
      {:else if (article?.error || articleCommandError) && !article?.rssContentHtml}
        <section class="fallback-notice" aria-live="polite">
          <span class="notice-icon"><Icon name="info" size={13} /></span>
          <div>
            <strong>{t('reader.rssVersionTitle')}</strong>
            <p>{article?.error ?? articleCommandError}</p>
            <button
              type="button"
              disabled={refreshingArticle || loadingArticle}
              onclick={refreshArticle}
            >
              {refreshingArticle ? t('reader.tryingAgain') : t('reader.tryFullTextAgain')}
            </button>
          </div>
        </section>
      {:else if loadingArticle && !fullArticleLoaded}
        <div class="article-loading">
          <Spinner size={17} />
          <div>
            <strong>{t('reader.loadingFullTitle')}</strong>
            <p>{t('reader.loadingFullBody')}</p>
          </div>
        </div>
      {/if}
      <!-- The article keeps its own language (line breaking, hyphenation,
           screen reader voice), independent of the UI language. -->
      <div bind:this={contentEl} lang={feed?.language ?? undefined}>
        <FeedContent
          content={article?.contentHtml ??
            stripSyndicationFooter(article?.rssContentHtml ?? item.summary)}
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
      {#if memos.length > 0}
        <section class="memo-list">
          <h2>{t('reader.memosTitle')}</h2>
          <ul>
            {#each memos as memo (memo.id)}
              <li>
                <button
                  type="button"
                  class="memo-list-item"
                  class:orphaned={orphanedAnnotationIds.includes(memo.id)}
                  onclick={() => scrollToAnnotation(memo.id)}
                >
                  <span class="memo-quote"
                    >“{memo.quotedText.slice(0, 80)}{memo.quotedText.length > 80 ? '…' : ''}”</span
                  >
                  <span class="memo-note">{memo.note}</span>
                  {#if orphanedAnnotationIds.includes(memo.id)}
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
{:else}
  <div class="empty-placeholder">
    <span class="empty-placeholder-icon"><Icon name="empty" size={20} /></span>
    <p>{t('reader.selectPost')}</p>
  </div>
{/if}

<style>
  .reader-scroll {
    height: 100%;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .reader {
    width: min(860px, 100%);
    margin-inline: auto;
    padding: clamp(20px, 5cqw, 40px);
  }

  .reader-header {
    margin-bottom: var(--space-5);
    padding-bottom: var(--space-4);
    border-bottom: 1px solid var(--color-border);
  }

  .header-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }

  .byline {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 6px;
    margin: 0;
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

  .header-buttons {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 2px;
  }

  .icon-button {
    display: grid;
    width: 30px;
    height: 30px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
  }

  .icon-button:hover:not(:disabled) {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  .icon-button.active {
    color: var(--color-accent);
  }

  .icon-button:disabled {
    cursor: default;
  }

  .icon-button.spinning :global(svg) {
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .fallback-notice,
  .article-loading {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin: 0 0 18px;
    padding: 12px 14px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg-secondary);
  }

  .notice-icon {
    display: grid;
    width: 20px;
    height: 20px;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 50%;
    background: var(--color-selection-bg);
    color: var(--color-accent);
    font-weight: var(--weight-bold);
  }

  .fallback-notice strong,
  .article-loading strong {
    color: var(--color-text);
    font-size: var(--text-body);
  }
  .fallback-notice p,
  .article-loading p {
    margin: 3px 0 8px;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.45;
  }
  .fallback-notice button {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-accent);
    font: inherit;
    font-size: var(--text-small);
    font-weight: var(--weight-bold);
    cursor: pointer;
  }
  .article-loading {
    align-items: center;
    border-color: color-mix(in srgb, var(--color-accent) 22%, var(--color-border));
  }
  .article-loading p {
    margin-bottom: 0;
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
    margin-top: 28px;
    padding-top: 18px;
    border-top: 1px solid var(--color-border);
  }

  .memo-list h2 {
    margin: 0 0 10px;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    font-weight: var(--weight-bold);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .memo-list ul {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .memo-list-item {
    display: flex;
    width: 100%;
    flex-direction: column;
    gap: 2px;
    padding: 10px 12px;
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

  .memo-quote {
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
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
    animation: aequo-flash 1.2s ease;
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
