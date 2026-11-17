<script lang="ts">
  import {
    createAnnotation,
    deleteAnnotation,
    listItemAnnotations,
    loadArticle,
    updateAnnotation,
  } from '$lib/api';
  import FeedContent from './FeedContent.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import Spinner from './Spinner.svelte';
  import { stripSyndicationFooter } from '$lib/format';
  import type { Annotation, ArticleView, FeedSummary, Item } from '$lib/types';
  import type { CreateAnnotationInput, ReanchoredUpdate } from './FeedContent.svelte';

  interface Props {
    item: Item | null;
    feeds: FeedSummary[];
    onClose: () => void;
    onToggleSaved: (itemId: number) => void;
    onAskAi?: (quotedText: string) => void;
  }

  let { item, feeds, onClose, onToggleSaved, onAskAi }: Props = $props();
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

  function formatTime(unixSecs: number): string {
    return new Date(unixSecs * 1000).toLocaleString(undefined, {
      dateStyle: 'long',
      timeStyle: 'short',
    });
  }

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
        <div class="header-main">
          <div class="heading">
            <p class="source">{feed?.title ?? feed?.url ?? 'Unknown feed'}</p>
            <h1>{item.title ?? '(untitled)'}</h1>
            <time datetime={new Date(item.publishedAt * 1000).toISOString()}>
              {formatTime(item.publishedAt)}
            </time>
          </div>
          <div class="header-buttons">
            <button class="save-reader" type="button" onclick={() => onToggleSaved(item.id)}
              ><Icon name={item.isSaved ? 'star' : 'star-outline'} size={14} />
              {item.isSaved ? 'Saved' : 'Save'}</button
            >
            <button class="close" type="button" aria-label="Close article" onclick={onClose}
              ><Icon name="close" size={16} /></button
            >
          </div>
        </div>
        {#if item.link}
          <div class="reader-actions">
            <div class="article-state" class:full={fullArticleLoaded}>
              <span class="state-dot"></span>
              {#if extractedArticleLoaded}Full article{:else if article?.rssContentHtml}Full text
                from RSS{:else if loadingArticle}Fetching full text…{:else}RSS preview{/if}
              {#if article?.cached && extractedArticleLoaded}<span class="cached-label"
                  >· cached</span
                >{/if}
            </div>
            {#if extractedArticleLoaded}
              <button
                class="refresh-article"
                type="button"
                disabled={refreshingArticle}
                onclick={refreshArticle}
              >
                {refreshingArticle ? 'Refreshing…' : 'Refresh'}
              </button>
            {/if}
            <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
            <a class="original-link" href={item.link} target="_blank" rel="noopener noreferrer">
              Read original site <Icon name="external-link" size={13} />
            </a>
          </div>
        {/if}
      </header>
      {#if (article?.error || articleCommandError) && extractedArticleLoaded}
        <section class="fallback-notice refresh-warning" aria-live="polite">
          <span class="notice-icon"><Icon name="info" size={13} /></span>
          <div>
            <strong>Showing the saved full text</strong>
            <p>{article?.error ?? articleCommandError}</p>
            <button type="button" disabled={refreshingArticle} onclick={refreshArticle}>
              {refreshingArticle ? 'Trying again…' : 'Try refresh again'}
            </button>
          </div>
        </section>
      {:else if (article?.error || articleCommandError) && !article?.rssContentHtml}
        <section class="fallback-notice" aria-live="polite">
          <span class="notice-icon"><Icon name="info" size={13} /></span>
          <div>
            <strong>Showing the RSS version</strong>
            <p>{article?.error ?? articleCommandError}</p>
            <button
              type="button"
              disabled={refreshingArticle || loadingArticle}
              onclick={refreshArticle}
            >
              {refreshingArticle ? 'Trying again…' : 'Try full text again'}
            </button>
          </div>
        </section>
      {:else if loadingArticle && !fullArticleLoaded}
        <div class="article-loading">
          <Spinner size={17} />
          <div>
            <strong>Loading the full article</strong>
            <p>Only requested because you opened this post.</p>
          </div>
        </div>
      {/if}
      <div bind:this={contentEl}>
        <FeedContent
          content={article?.contentHtml ?? stripSyndicationFooter(article?.rssContentHtml ?? item.summary)}
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
          <h2>This post's memos</h2>
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
                    <span class="memo-orphaned-badge">Position not found</span>
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
    <p>Select a post to read it.</p>
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
    margin-bottom: 20px;
    padding-bottom: 18px;
    border-bottom: 1px solid var(--color-border);
  }

  .header-buttons {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .save-reader {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 34px;
    padding: 6px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-body);
    cursor: pointer;
    white-space: nowrap;
  }
  .save-reader:hover {
    color: var(--color-accent);
  }

  .header-main {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 14px;
  }

  .heading {
    min-width: 0;
  }

  .reader-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 14px;
  }

  .article-state {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-right: auto;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .article-state.full {
    color: var(--color-success);
  }
  .state-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-text-tertiary);
  }
  .article-state.full .state-dot {
    background: var(--color-success);
  }
  .cached-label {
    opacity: 0.75;
  }

  .refresh-article {
    min-height: 34px;
    padding: 7px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  .refresh-article:hover:not(:disabled) {
    border-color: var(--color-accent);
    color: var(--color-accent);
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

  .source {
    margin: 0 0 8px;
    color: var(--color-accent);
    font-size: var(--text-small);
    font-weight: var(--weight-bold);
  }

  h1 {
    margin: 0 0 10px;
    color: var(--color-text);
    font-size: var(--text-display);
    font-weight: var(--weight-bold);
    letter-spacing: -0.025em;
    line-height: 1.25;
    word-break: keep-all;
  }

  time {
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .original-link {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
    min-height: 38px;
    padding: 8px 12px;
    border-radius: var(--radius-md);
    border: 0;
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
    font: inherit;
    font-size: var(--text-body);
    font-weight: var(--weight-bold);
    text-decoration: none;
    cursor: pointer;
  }

  .original-link:hover {
    filter: brightness(0.97);
  }

  .close {
    display: grid;
    width: 34px;
    height: 34px;
    place-items: center;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    cursor: pointer;
  }

  .close:hover {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  @container (max-width: 420px) {
    .header-main {
      flex-direction: column-reverse;
    }

    .header-buttons {
      width: 100%;
      justify-content: flex-end;
    }
  }

  @container (max-width: 540px) {
    .reader-actions {
      justify-content: flex-start;
    }
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
