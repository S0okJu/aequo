<script lang="ts">
  import FeedContent from './FeedContent.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import Spinner from './Spinner.svelte';
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
    onSelect: (itemId: number) => void;
    onToggleSaved: (itemId: number) => void;
    onEndReached: () => void;
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
    onSelect,
    onToggleSaved,
    onEndReached,
  }: Props = $props();

  const selectedFeed = $derived(feeds.find((feed) => feed.id === selectedFeedId));
  const selectedFolder = $derived(folders.find((folder) => folder.id === selectedFolderId));
  const viewLabel = $derived(
    selectedStarred
      ? 'STARRED'
      : selectedToday
        ? 'TODAY'
        : selectedFeed
          ? 'FEED'
          : selectedFolder
            ? 'FOLDER'
            : 'ALL',
  );
  const viewTitle = $derived(
    selectedStarred
      ? 'Starred'
      : selectedToday
        ? 'Today'
        : selectedFeed
          ? (selectedFeed.title ?? selectedFeed.url)
          : selectedFolder
            ? selectedFolder.name
            : 'All posts',
  );
  const unreadInView = $derived(items.filter((item) => !item.isRead).length);

  function feedFor(feedId: number): FeedSummary | undefined {
    return feeds.find((feed) => feed.id === feedId);
  }

  function formatTime(unixSecs: number): string {
    return new Date(unixSecs * 1000).toLocaleString(undefined, {
      dateStyle: 'short',
      timeStyle: 'short',
    });
  }

  function handleScroll(event: Event) {
    const element = event.currentTarget as HTMLDivElement;
    if (element.scrollTop + element.clientHeight >= element.scrollHeight - 500) onEndReached();
  }
</script>

<div class="posts-scroll" onscroll={handleScroll}>
  <div class="posts">
    <div class="feed-context">
      <span class="feed-context-label">{viewLabel}</span>
      <strong>{viewTitle}</strong>
      {#if unreadInView > 0}<span class="unread-pill">{unreadInView} unread</span>{/if}
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
          <h1>No starred posts</h1>
          <p>Star a post to come back to it later - it'll show up here.</p>
        {:else}
          <h1>No posts yet</h1>
          <p>New posts from your subscriptions will show up here.</p>
        {/if}
      </div>
    {:else}
      {#each items as item, index (item.id)}
        {@const day = new Date(item.publishedAt * 1000)}
        {@const previous = index > 0 ? new Date(items[index - 1].publishedAt * 1000) : null}
        {#if !previous || day.toDateString() !== previous.toDateString()}
          <h2 class="date-heading">
            {day.toLocaleDateString(undefined, {
              weekday: 'long',
              year: 'numeric',
              month: 'long',
              day: 'numeric',
            })}
          </h2>
        {/if}
        {@const feed = feedFor(item.feedId)}
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
            <span class="avatar" aria-hidden="true">
              {(feed?.title ?? feed?.url ?? '?').slice(0, 1).toUpperCase()}
            </span>
            <span class="byline">
              <span class="source">{feed?.title ?? feed?.url ?? 'Unknown feed'}</span>
              <time datetime={new Date(item.publishedAt * 1000).toISOString()}>
                {formatTime(item.publishedAt)}
              </time>
            </span>
            {#if !item.isRead}<span class="unread-dot" aria-label="Unread"></span>{/if}
            <button
              class="save-button"
              type="button"
              aria-label={item.isSaved ? 'Unsave post' : 'Save post'}
              title={item.isSaved ? 'Saved' : 'Save'}
              onclick={(event) => {
                event.stopPropagation();
                onToggleSaved(item.id);
              }}><Icon name={item.isSaved ? 'star' : 'star-outline'} size={15} /></button
            >
          </span>
          <span class="title">{item.title ?? '(untitled)'}</span>
          <FeedContent
            content={item.summary ?? item.fullContent}
            baseUrl={item.link ?? feed?.url ?? ''}
            compact
          />
        </div>
      {/each}
      {#if hasMore}
        <p class="loading-more">Scroll for more</p>
      {/if}
    {/if}
  </div>
</div>

<style>
  .posts-scroll {
    height: 100%;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .posts {
    display: flex;
    min-height: 100%;
    flex-direction: column;
    max-width: var(--pane-content-max-width);
    margin: 0 auto;
    padding: 10px;
  }

  .posts > .empty-placeholder {
    flex: 1;
  }

  .feed-context {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 2px 2px 12px;
    padding: 4px 3px 10px;
    border-bottom: 1px solid var(--color-border);
  }
  .feed-context-label {
    flex-shrink: 0;
    color: var(--color-text-tertiary);
    font-size: 0.68em;
    font-weight: 700;
    letter-spacing: 0.07em;
  }
  .feed-context strong {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: var(--color-text-secondary);
    font-size: 0.82em;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .unread-pill {
    flex-shrink: 0;
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--color-accent) 12%, var(--color-bg));
    color: var(--color-accent);
    font-size: 0.72em;
    font-weight: 650;
  }

  .post {
    display: block;
    width: 100%;
    margin-bottom: 8px;
    padding: 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background-color var(--transition),
      border-color var(--transition);
    /* Skip layout/style/paint for off-screen cards (variable height, so no
       JS windowing - see docs/IMPLEMENTATION_PLAN.md perf notes) instead of
       mounting every loaded post at once. 230px is only a guess used before
       a card has ever been laid out; `auto` tells engines that support it
       (current WebKit/Chromium/Gecko) to remember each card's real height
       once measured, instead of always using the guess - without this, the
       guess vs. real-height mismatch understates/overstates scrollHeight
       and leaves stray blank space once you scroll to the true end. The
       bare-length line is a no-op fallback for older engines that don't
       parse the `auto <length>` syntax: an unparseable `auto` declaration
       is dropped, leaving this one in effect (unchanged old behavior, not
       a regression there). */
    content-visibility: auto;
    contain-intrinsic-size: 0 230px;
    contain-intrinsic-size: auto 230px;
  }

  .date-heading {
    margin: 20px 3px 9px;
    color: var(--color-text-secondary);
    font-size: 0.78em;
    font-weight: 700;
    content-visibility: auto;
    contain-intrinsic-size: 0 40px;
    contain-intrinsic-size: auto 40px;
  }

  .post:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  .save-button {
    display: inline-flex;
    align-items: center;
    border: 0;
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
  }
  .save-button {
    justify-content: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
  }
  .save-button:hover {
    color: var(--color-accent);
  }

  .post:hover {
    background: var(--color-hover-bg);
  }

  .post.selected {
    border-color: var(--color-accent);
    background: var(--color-selection-bg);
  }

  .post.unread .title {
    font-weight: 650;
  }

  .post-header {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    margin-bottom: 8px;
  }

  .avatar {
    display: grid;
    flex-shrink: 0;
    width: 27px;
    height: 27px;
    place-items: center;
    border-radius: 50%;
    background: var(--color-bg-secondary);
    color: var(--color-text-secondary);
    font-size: 0.76em;
    font-weight: 650;
  }

  .byline {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
  }

  .source {
    overflow: hidden;
    color: var(--color-text-secondary);
    font-size: 0.75em;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  time {
    color: var(--color-text-tertiary);
    font-size: 0.7em;
  }

  .unread-dot {
    width: 7px;
    height: 7px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--color-accent);
  }

  .title {
    display: block;
    display: -webkit-box;
    margin-bottom: 6px;
    overflow: hidden;
    font-size: 1em;
    font-weight: 650;
    line-height: 1.4;
    word-break: keep-all;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
  }

  .loading-more {
    font-size: 0.8em;
  }

  .loading-more {
    padding: 10px;
    color: var(--color-text-tertiary);
    text-align: center;
  }
</style>
