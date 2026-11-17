<script lang="ts">
  import { onMount } from 'svelte';
  import FeedContent from './FeedContent.svelte';
  import FeedIcon from './FeedIcon.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import Spinner from './Spinner.svelte';
  import {
    formatDateTime,
    formatDayHeading,
    formatRelative,
    shortFeedTitle,
    stripSyndicationFooter,
  } from '$lib/format';
  import { t } from '$lib/i18n/index.svelte';
  import type { FeedFolder, FeedSummary, Item } from '$lib/types';

  interface Props {
    items: Item[];
    feeds: FeedSummary[];
    folders: FeedFolder[];
    loading?: boolean;
    selectedFeedId: number | null;
    selectedFolderId: number | null;
    selectedToday?: boolean;
    selectedStarred?: boolean;
    selectedItemId: number | null;
    hasMore: boolean;
    unreadOnly?: boolean;
    /** Unread total for the whole view (all pages); falls back to the loaded items. */
    unreadCount?: number;
    onSelect: (itemId: number) => void;
    onToggleSaved: (itemId: number) => void;
    onEndReached: () => void;
    onUnreadOnlyChange?: (unreadOnly: boolean) => void;
    onMarkAllRead?: () => void;
  }

  let {
    items,
    feeds,
    folders,
    loading = false,
    selectedFeedId,
    selectedFolderId,
    selectedToday = false,
    selectedStarred = false,
    selectedItemId,
    hasMore,
    unreadOnly = false,
    unreadCount,
    onSelect,
    onToggleSaved,
    onEndReached,
    onUnreadOnlyChange,
    onMarkAllRead,
  }: Props = $props();

  const selectedFeed = $derived(feeds.find((feed) => feed.id === selectedFeedId));
  const selectedFolder = $derived(folders.find((folder) => folder.id === selectedFolderId));
  const viewLabel = $derived(
    t(
      selectedStarred
        ? 'posts.label.starred'
        : selectedToday
          ? 'posts.label.today'
          : selectedFeed
            ? 'posts.label.feed'
            : selectedFolder
              ? 'posts.label.folder'
              : 'posts.label.all',
    ),
  );
  const viewTitle = $derived(
    selectedStarred
      ? t('posts.title.starred')
      : selectedToday
        ? t('posts.title.today')
        : selectedFeed
          ? shortFeedTitle(selectedFeed.title, selectedFeed.url)
          : selectedFolder
            ? selectedFolder.name
            : t('posts.title.all'),
  );
  const unreadInView = $derived(unreadCount ?? items.filter((item) => !item.isRead).length);

  function feedFor(feedId: number): FeedSummary | undefined {
    return feeds.find((feed) => feed.id === feedId);
  }

  // Shared clock for the relative timestamps ("5 min. ago"), ticking once a
  // minute so they don't go stale while the list stays open.
  let nowSecs = $state(Date.now() / 1000);
  onMount(() => {
    const timer = setInterval(() => (nowSecs = Date.now() / 1000), 60_000);
    return () => clearInterval(timer);
  });

  function handleScroll(event: Event) {
    const element = event.currentTarget as HTMLDivElement;
    if (hasMore && element.scrollTop + element.clientHeight >= element.scrollHeight - 500) {
      onEndReached();
    }
  }
</script>

<div class="posts-scroll" onscroll={handleScroll}>
  <div class="posts">
    <div class="feed-context">
      <span class="feed-context-label">{viewLabel}</span>
      <strong>{viewTitle}</strong>
      {#if unreadInView > 0}<span class="unread-count"
          >{t('posts.unreadCount', { count: unreadInView })}</span
        >{/if}
      {#if onUnreadOnlyChange && !selectedStarred}
        <div class="segmented" role="group" aria-label={t('posts.filter.label')}>
          <button type="button" aria-pressed={!unreadOnly} onclick={() => onUnreadOnlyChange(false)}
            >{t('posts.filter.all')}</button
          >
          <button type="button" aria-pressed={unreadOnly} onclick={() => onUnreadOnlyChange(true)}
            >{t('posts.filter.unread')}</button
          >
        </div>
      {/if}
      {#if onMarkAllRead}
        <button
          type="button"
          class="toolbar-button"
          aria-label={t('posts.markAllRead')}
          title={t('posts.markAllRead')}
          disabled={unreadInView === 0}
          onclick={onMarkAllRead}><Icon name="check-all" size={15} /></button
        >
      {/if}
    </div>
    {#if loading && items.length === 0}
      <div class="empty-placeholder">
        <Spinner size={20} />
      </div>
    {:else if items.length === 0}
      <div class="empty-placeholder">
        <span class="empty-placeholder-icon"
          ><Icon name={selectedStarred ? 'star-outline' : 'empty'} size={20} /></span
        >
        {#if selectedStarred}
          <h1>{t('posts.empty.starredTitle')}</h1>
          <p>{t('posts.empty.starredBody')}</p>
        {:else if unreadOnly}
          <h1>{t('posts.empty.caughtUpTitle')}</h1>
          <p>{t('posts.empty.caughtUpBody')}</p>
        {:else}
          <h1>{t('posts.empty.noneTitle')}</h1>
          <p>{t('posts.empty.noneBody')}</p>
        {/if}
      </div>
    {:else}
      {#each items as item, index (item.id)}
        {@const day = new Date(item.publishedAt * 1000)}
        {@const previous = index > 0 ? new Date(items[index - 1].publishedAt * 1000) : null}
        {#if !previous || day.toDateString() !== previous.toDateString()}
          <h2 class="date-heading">{formatDayHeading(item.publishedAt, nowSecs)}</h2>
        {/if}
        {@const feed = feedFor(item.feedId)}
        {@const feedName = shortFeedTitle(feed?.title, feed?.url ?? t('common.unknownFeed'))}
        <div
          class="post"
          class:selected={selectedItemId === item.id}
          class:unread={!item.isRead}
          role="button"
          tabindex="0"
          onclick={() => onSelect(item.id)}
          onkeydown={(event) => {
            if (
              event.target === event.currentTarget &&
              (event.key === 'Enter' || event.key === ' ')
            ) {
              event.preventDefault();
              onSelect(item.id);
            }
          }}
        >
          <span class="post-header">
            <FeedIcon {feed} />
            <span class="source">{feedName}</span>
            <span class="separator" aria-hidden="true">·</span>
            <time
              datetime={new Date(item.publishedAt * 1000).toISOString()}
              title={formatDateTime(item.publishedAt)}
            >
              {formatRelative(item.publishedAt, nowSecs)}
            </time>
            {#if !item.isRead}<span class="visually-hidden">{t('posts.unreadMarker')}</span>{/if}
            <button
              class="save-button"
              class:saved={item.isSaved}
              type="button"
              aria-label={item.isSaved ? t('common.unsavePost') : t('common.savePost')}
              title={item.isSaved ? t('common.saved') : t('common.save')}
              onclick={(event) => {
                event.stopPropagation();
                onToggleSaved(item.id);
              }}><Icon name={item.isSaved ? 'star' : 'star-outline'} size={14} /></button
            >
          </span>
          <span class="title">{item.title ?? '(untitled)'}</span>
          <FeedContent
            content={stripSyndicationFooter(item.summary ?? item.fullContent)}
            baseUrl={item.link ?? feed?.url ?? ''}
            compact
          />
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .posts-scroll {
    height: 100%;
    overflow-y: auto;
    overscroll-behavior: contain;
    container-type: inline-size;
  }

  /* A narrow list (reader + AI open) keeps the view title readable by
     dropping the secondary label and count before squeezing the title. */
  @container (max-width: 440px) {
    .feed-context-label,
    .unread-count {
      display: none;
    }
  }

  .posts {
    display: flex;
    min-height: 100%;
    flex-direction: column;
    max-width: var(--pane-content-max-width);
    margin: 0 auto;
    padding: var(--space-2) var(--space-2) var(--space-5);
  }

  .posts > .empty-placeholder {
    flex: 1;
  }

  .feed-context {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3) var(--space-3);
  }
  .feed-context-label {
    flex-shrink: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    font-weight: var(--weight-bold);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .feed-context strong {
    min-width: 5em;
    flex: 1;
    overflow: hidden;
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .unread-count {
    flex-shrink: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .segmented {
    display: inline-flex;
    flex-shrink: 0;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--color-hover-bg);
  }
  .segmented button {
    padding: 2px var(--space-2);
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }
  .segmented button[aria-pressed='true'] {
    background: var(--color-bg);
    color: var(--color-text);
    font-weight: var(--weight-medium);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .toolbar-button {
    display: inline-flex;
    width: 26px;
    height: 26px;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
  }
  .toolbar-button:hover:not(:disabled) {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }
  .toolbar-button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .date-heading {
    margin: var(--space-4) var(--space-3) var(--space-1);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    font-weight: var(--weight-medium);
    content-visibility: auto;
    contain-intrinsic-size: 0 32px;
    contain-intrinsic-size: auto 32px;
  }

  /* A flat row separated by a hairline instead of a bordered card - the
     unread state is carried by title weight, not a colored dot. */
  .post {
    position: relative;
    display: block;
    width: 100%;
    padding: var(--space-3);
    border-radius: var(--radius-md);
    color: var(--color-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--transition);
    /* Skip layout/style/paint for off-screen rows (variable height, so no
       JS windowing - see docs/IMPLEMENTATION_PLAN.md perf notes) instead of
       mounting every loaded post at once. 110px is only a guess used before
       a row has ever been laid out; `auto` tells engines that support it
       to remember each row's real height once measured. The bare-length
       line is a fallback for engines that don't parse `auto <length>`. */
    content-visibility: auto;
    contain-intrinsic-size: 0 110px;
    contain-intrinsic-size: auto 110px;
  }

  .post + .post::before {
    position: absolute;
    top: 0;
    right: var(--space-3);
    left: var(--space-3);
    height: 1px;
    background: var(--color-border);
    content: '';
  }

  .post:hover {
    background: var(--color-hover-bg);
  }

  .post.selected {
    background: var(--color-selection-bg);
  }

  /* Hide the hairline on either side of a highlighted row so the hover /
     selection fill reads as one shape. */
  .post:hover::before,
  .post.selected::before,
  .post:hover + .post::before,
  .post.selected + .post::before {
    display: none;
  }

  .post:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
  }

  .post-header {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 20px;
    margin-bottom: var(--space-1);
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .source {
    min-width: 0;
    overflow: hidden;
    color: var(--color-text-secondary);
    font-weight: var(--weight-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .separator,
  time {
    flex-shrink: 0;
  }

  .save-button {
    display: inline-flex;
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    margin-left: auto;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
    opacity: 0;
  }

  /* Only a saved post shows its star at rest; others reveal it on hover or
     keyboard focus so the control stays reachable. */
  .save-button.saved,
  .post:hover .save-button,
  .post:focus-within .save-button,
  .post.selected .save-button {
    opacity: 1;
  }

  .save-button:hover,
  .save-button.saved {
    color: var(--color-accent);
  }

  .title {
    display: block;
    display: -webkit-box;
    margin-bottom: 2px;
    overflow: hidden;
    color: var(--color-text-secondary);
    font-size: var(--text-title);
    font-weight: var(--weight-regular);
    line-height: 1.4;
    word-break: keep-all;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }

  .post.unread .title {
    color: var(--color-text);
    font-weight: var(--weight-bold);
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
