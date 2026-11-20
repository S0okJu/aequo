<script lang="ts">
  import { updateDraft } from '$lib/updates.svelte';
  import { onMount, tick, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { listen } from '@tauri-apps/api/event';
  import {
    cancelAiMessage,
    deleteAiSession,
    listAiSessions,
    listItems,
    listAllAiSessions,
    addAiContextItems,
    removeAiContextItem,
    loadAiTranscript,
    startAiMessage,
  } from '$lib/api';
  import { ConversationContext } from '$lib/ai/context.svelte';
  import AiContextChips from './AiContextChips.svelte';
  import AiArticleMentions from './AiArticleMentions.svelte';
  import { articleMentionAt, type ArticleMention } from '$lib/ai/mentions';
  import AiProviderPicker from './AiProviderPicker.svelte';
  import SegmentedControl from './SegmentedControl.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import IconButton from './IconButton.svelte';
  import ErrorCard from './ErrorCard.svelte';
  import ThinkingDots from './ThinkingDots.svelte';
  import AiSuggestionBar from './AiSuggestionBar.svelte';
  import { clickOutside } from '$lib/clickOutside';
  import { formatRelative } from '$lib/format';
  import { t, type MessageKey } from '$lib/i18n/index.svelte';
  import { renderMarkdown } from '$lib/markdown';
  import { resourceSettings } from '$lib/resources.svelte';
  import { TranscriptWindow } from '$lib/transcript.svelte';
  import type {
    AiMessage,
    AiProvider,
    AiSession,
    AiStreamEvent,
    Item,
    AiContextRequest,
    FeedSummary,
    ItemFilter,
  } from '$lib/types';

  interface Props {
    item: Item | null;
    standalone?: boolean;
    contextRequest?: AiContextRequest | null;
    feeds?: FeedSummary[];
    onOpenItem?: (id: number) => void;
    onPinnedChange?: (pinned: boolean) => void;
    onBusyChange?: (busy: boolean) => void;
    onHistoryChange?: () => void;
    onCollapse: () => void;
    pendingQuote?: string | null;
    onConsumedPendingQuote?: () => void;
  }

  let {
    item,
    standalone = false,
    contextRequest = null,
    feeds = [],
    onOpenItem,
    onPinnedChange,
    onBusyChange,
    onHistoryChange,
    onCollapse,
    pendingQuote = null,
    onConsumedPendingQuote,
  }: Props = $props();

  const PROVIDER_KEY = 'aequo:aiProvider';
  const PROVIDER_NAMES: Record<AiProvider, string> = { codex: 'Codex', claude: 'Claude Code' };
  const PROVIDER_ICONS: Record<AiProvider, 'provider-codex' | 'provider-claude'> = {
    codex: 'provider-codex',
    claude: 'provider-claude',
  };

  function readStoredProvider(): AiProvider {
    try {
      return localStorage.getItem(PROVIDER_KEY) === 'claude' ? 'claude' : 'codex';
    } catch {
      return 'codex';
    }
  }

  /** The in-flight answer for the open conversation (one at a time). */
  interface Run {
    requestId: string | null;
    prompt: string;
    text: string;
    status: string | null;
  }

  let provider = $state<AiProvider>(readStoredProvider());
  let sessions = $state<AiSession[]>([]);
  const context = new ConversationContext();
  const transcript = new TranscriptWindow();
  const messages = $derived(transcript.messages);
  const citations = $derived(
    new Map(
      context.items.flatMap((i) =>
        i.itemId === null
          ? []
          : [[i.position + 1, { itemId: i.itemId, title: i.itemTitle }] as const],
      ),
    ),
  );
  /** Re-highlights an `@article` reference in a sent message the same way the composer shows it. */
  const mentionLabels = $derived(
    new Map(
      context.items.flatMap((i) =>
        i.itemId === null ? [] : [[articleLabel(i.itemTitle), i.itemId] as const],
      ),
    ),
  );
  function sourceLinks(node: HTMLElement) {
    const handle = (event: MouseEvent) => {
      const link = (event.target as Element)?.closest?.('a[data-article-id]');
      if (!link) return;
      event.preventDefault();
      const id = Number(link.getAttribute('data-article-id'));
      if (context.contains(id)) onOpenItem?.(id);
    };
    node.addEventListener('click', handle);
    return {
      destroy() {
        node.removeEventListener('click', handle);
      },
    };
  }
  let selectedSessionId = $state<number | null>(null);
  let draft = $state('');
  const updateDraftToken = Symbol('ai-draft');
  $effect(() => {
    updateDraft(updateDraftToken, draft.trim().length > 0);
  });
  $effect(() => () => updateDraft(updateDraftToken, false));
  let run = $state<Run | null>(null);
  let loadingMessages = $state(false);
  let updatingContext = $state(false);
  let error = $state<string | null>(null);
  // Set only alongside a classified CLI error (see `finishRun`'s `error`
  // branch); every other `setError` call clears it, so a later, unrelated
  // error never inherits a stale classification's title (§B).
  let errorKind = $state<AiStreamEvent['errorKind'] | null>(null);
  const errorTitle = $derived.by(() => {
    switch (errorKind) {
      case 'loginRequired':
        return t('ai.error.loginRequired');
      case 'rateLimit':
        return t('ai.error.rateLimit');
      case 'network':
        return t('ai.error.network');
      default:
        return t('ai.errorTitle');
    }
  });
  function setError(message: string) {
    errorKind = null;
    error = message;
  }
  let notice = $state<string | null>(null);
  const transcriptNote = $derived(transcript.note);
  let loadingPage = $state<'older' | 'newer' | 'latest' | null>(null);
  let disposed = false;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let lastPrompt = $state('');
  let copiedMessageId = $state<number | null>(null);
  let historyOpen = $state(false);
  let confirmingRemoveId = $state<number | null>(null);
  // Set after switching provider mid-conversation: lets the user jump back.
  let previousConversation = $state<{ provider: AiProvider; sessionId: number } | null>(null);
  // Awaiting confirmation before switching providers mid-conversation (§D).
  let pendingProviderSwitch = $state<AiProvider | null>(null);
  // Bumped on cancel to force the <select> back to `provider` (see
  // AiProviderPicker's `resetKey`) even though `provider` itself never changed.
  let providerPickerResetKey = $state(0);
  // The conversation reopened for the post that just became visible. Drives
  // the "previous conversation / new conversation" divider only.
  let resumedSession = $state<AiSession | null>(null);
  let liveMessage = $state('');
  let historyTab = $state<'item' | 'all'>('item');
  let allSessions = $state<AiSession[]>([]);
  const visibleSessions = $derived(
    (historyTab === 'all' ? allSessions : sessions).toSorted(
      (a, b) =>
        Number(b.id === selectedSessionId) - Number(a.id === selectedSessionId) ||
        b.updatedAt - a.updatedAt,
    ),
  );
  $effect(() => {
    onPinnedChange?.(context.pinned);
  });
  $effect(() => {
    onBusyChange?.(sending || loadingMessages || updatingContext);
  });
  /** Posts the user waved off in the suggestion bar; cleared on a new chat. */
  const dismissedSuggestions = new SvelteSet<number>();
  let messageList = $state<HTMLDivElement | undefined>(undefined);
  let composerEl = $state<HTMLTextAreaElement | undefined>(undefined);
  let composerBackdropEl = $state<HTMLDivElement | undefined>(undefined);
  let contextGeneration = 0;
  let loadGeneration = 0;
  let nextLocalId = -1;

  const componentId = $props.id();
  const mentionListId = `${componentId}-mentions`;
  let mention = $state<ArticleMention | null>(null);
  let mentionPicker = $state<{ handleKeydown: (event: KeyboardEvent) => boolean }>();
  let activeMentionId = $state<string | undefined>();
  /** An `@article` or a `"quoted excerpt"` reference still spelled out in the
   * current draft; cleared once it's sent. Deleting its text in the composer
   * drops the reference (see `reconcileDraftReferences`), same as adding it. */
  type DraftReference =
    | { kind: 'article'; text: string; itemId: number }
    | { kind: 'quote'; text: string; full: string };
  let draftReferences = $state<DraftReference[]>([]);

  /**
   * A textarea can't ellipsize mid-string like the context chip does, so this
   * cuts at a word boundary and keeps enough of the headline to still
   * identify the article at a glance, rather than a generic truncated prefix.
   */
  function truncateAtWord(value: string, limit: number): string {
    if (value.length <= limit) return value;
    const cut = value.slice(0, limit);
    const lastSpace = cut.lastIndexOf(' ');
    const short = lastSpace > limit / 2 ? cut.slice(0, lastSpace) : cut;
    return `${short.trimEnd()}…`;
  }

  /** Shared by the composer (an `Item`) and by re-labeling a sent message from `context.items` (a bare title). */
  function articleLabel(title: string | null | undefined): string {
    return `@${truncateAtWord(title?.trim() || t('ai.mention.untitled'), 40)}`;
  }

  function mentionLabel(article: Item): string {
    return articleLabel(article.title);
  }

  /** The inline label shown for a quoted excerpt; the full text still goes to the AI (see `resolveQuoteReferences`). */
  function quoteLabel(excerpt: string): string {
    return `"${truncateAtWord(excerpt.trim().replace(/\s+/g, ' '), 40)}"`;
  }

  /**
   * Swaps each `"quote"` reference's short label for its full excerpt before
   * the prompt is sent - the composer only ever shows the shortened label.
   * One sitting alone at the start of the message becomes a proper markdown
   * blockquote (matching how quoting used to work); anywhere else, it's
   * spliced in as a plain quoted phrase.
   */
  function resolveQuoteReferences(text: string): string {
    let prompt = text;
    for (const ref of draftReferences) {
      if (ref.kind !== 'quote') continue;
      const at = prompt.indexOf(ref.text);
      if (at === -1) continue;
      const before = prompt.slice(0, at);
      const after = prompt.slice(at + ref.text.length);
      prompt =
        before.trim() === ''
          ? `${ref.full.replace(/^/gm, '> ')}\n\n${after.replace(/^\s+/, '')}`
          : `${before}"${ref.full}"${after}`;
    }
    return prompt;
  }

  function escapeComposerHtml(value: string): string {
    return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  /**
   * Renders the draft behind the (now text-transparent) textarea, wrapping
   * each still-tracked `@article`/`"quote"` reference in a highlighted span -
   * the textarea contributes only its caret and selection, this contributes
   * every visible glyph, so both must wrap identically (see the matching CSS).
   */
  function highlightedComposerHtml(text: string, references: { text: string }[]): string {
    const ranges = references
      .map((r) => {
        const start = text.indexOf(r.text);
        return start === -1 ? null : { start, end: start + r.text.length };
      })
      .filter((r): r is { start: number; end: number } => r !== null)
      .sort((a, b) => a.start - b.start);
    let html = '';
    let cursor = 0;
    for (const range of ranges) {
      if (range.start < cursor) continue;
      html += escapeComposerHtml(text.slice(cursor, range.start));
      html += `<span class="mention-highlight">${escapeComposerHtml(text.slice(range.start, range.end))}</span>`;
      cursor = range.end;
    }
    html += escapeComposerHtml(text.slice(cursor));
    // A lone trailing newline collapses to no visible height in HTML, but a
    // textarea still reserves a line for it - keep them the same height.
    return `${html}\n`;
  }

  function syncComposerScroll() {
    if (composerBackdropEl && composerEl) composerBackdropEl.scrollTop = composerEl.scrollTop;
  }

  function syncMention() {
    if (!composerEl || sending || loadingMessages || updatingContext) {
      mention = null;
      return;
    }
    mention = articleMentionAt(
      composerEl.value,
      composerEl.selectionStart,
      composerEl.selectionEnd,
    );
  }

  /** The empty-state "@mention an article" chip: types "@" into the composer
   * and opens the same picker a user typing it themselves would see. */
  async function startMentionHint() {
    if (sending || loadingMessages || updatingContext) return;
    draft = '@';
    await tick();
    composerEl?.focus();
    composerEl?.setSelectionRange(draft.length, draft.length);
    syncMention();
    resizeComposer();
  }

  async function chooseMention(article: Item): Promise<boolean> {
    const token = mention;
    if (!token || sending || loadingMessages || updatingContext) return false;
    const original = draft.slice(token.start, token.end);
    if (!context.contains(article.id) && !(await addItems([article]))) return false;
    context.pinned = true;
    notice = null;
    if (draft.slice(token.start, token.end) === original) {
      const inserted = `${mentionLabel(article)} `;
      const caret = composerEl?.selectionStart ?? token.end;
      draft = draft.slice(0, token.start) + inserted + draft.slice(token.end);
      draftReferences = [
        ...draftReferences,
        { kind: 'article', text: inserted.trim(), itemId: article.id },
      ];
      mention = null;
      await tick();
      composerEl?.focus();
      const delta = inserted.length - original.length;
      const nextCaret = caret >= token.end ? caret + delta : token.start + inserted.length;
      composerEl?.setSelectionRange(nextCaret, nextCaret);
      resizeComposer();
    } else mention = null;
    return true;
  }

  /**
   * Drops any `@article`/`"quote"` reference whose text the user deleted
   * from the draft. An article reference also un-pins the matching article;
   * a quote reference has nothing else to undo.
   */
  async function reconcileDraftReferences() {
    const gone = draftReferences.filter((r) => !draft.includes(r.text));
    if (gone.length === 0) return;
    draftReferences = draftReferences.filter((r) => draft.includes(r.text));
    for (const r of gone) {
      if (r.kind !== 'article') continue;
      const position = context.items.find((i) => i.itemId === r.itemId)?.position;
      if (position !== undefined) await removeContext(position);
    }
  }

  // Buffer outside reactive state; the selected resource policy controls
  // display updates independently from provider token delivery.
  let streamBuffer = '';
  let flushTimer: ReturnType<typeof setTimeout> | undefined;
  // Events can arrive before start_ai_message has returned the request id.
  let earlyEvents: AiStreamEvent[] = [];
  let earlyEventBytes = 0;
  let earlyOverflow = false;
  let streamBytes = 0;
  const encoder = new TextEncoder();
  const MAX_STREAM_BYTES = 1024 * 1024;
  let stickToBottom = true;

  // A run pins its conversation, and a pinned conversation stays on screen,
  // so there is no longer a case where the panel shows one conversation while
  // a run belongs to another. The stop button always controls what is visible.
  const sending = $derived(run !== null);
  const isEmptyChat = $derived(
    !selectedSessionId && messages.length === 0 && !sending && !loadingMessages,
  );
  /** The open post is not what this conversation is about, and could be. */
  const otherPostOpen = $derived(
    !standalone &&
      context.pinned &&
      item !== null &&
      !context.contains(item.id) &&
      !dismissedSuggestions.has(item.id),
  );

  async function loadAllSessions(postId: number): Promise<AiSession[]> {
    const [codexSessions, claudeSessions] = await Promise.all([
      listAiSessions(postId, 'codex'),
      listAiSessions(postId, 'claude'),
    ]);
    return [...codexSessions, ...claudeSessions].sort(
      (a, b) => b.updatedAt - a.updatedAt || b.id - a.id,
    );
  }

  // `untrack` keeps `item?.id` the only thing this effect depends on. The
  // body writes state that `pinned` is maintained from, so reading `pinned`
  // as a dependency would make the effect retrigger itself.
  $effect(() => {
    const postId = item?.id;
    const independent = standalone;
    untrack(() => applyOpenPostChange(postId, independent));
  });

  /**
   * A conversation the user has invested in survives the reader moving on; an
   * untouched one follows the open post, reopening that post's most recent
   * conversation or starting empty.
   */
  function applyOpenPostChange(postId: number | undefined, independent = standalone) {
    const generation = ++contextGeneration;

    if (independent || context.pinned) {
      // Only the history list is post-scoped. The conversation, its anchor,
      // its run and its transcript all stay exactly as they are.
      sessions = [];
      historyOpen = false;
      if (postId === undefined) return;
      loadAllSessions(postId)
        .then((result) => {
          if (generation === contextGeneration) sessions = result;
        })
        .catch((reason) => {
          if (generation === contextGeneration) setError(String(reason));
        });
      return;
    }

    const load = (loadGeneration += 1);
    loadingMessages = false;
    loadingPage = null;
    run = null;
    selectedSessionId = null;
    transcript.clear();
    sessions = [];
    error = null;
    notice = null;
    transcript.note = null;
    historyOpen = false;
    previousConversation = null;
    resumedSession = null;
    context.reset(item ? [item] : []);
    if (postId === undefined) return;
    loadAllSessions(postId)
      .then((result) => {
        if (generation !== contextGeneration) return;
        sessions = result;
        // Restore the most recent conversation, adopting its provider.
        const candidate = result[0];
        // A `startNewChat()` in between (the "Ask AI about selection" effect
        // below, or the user's own click) wins: it bumped `loadGeneration`.
        if (!candidate || load !== loadGeneration) return;
        openSession(candidate, true);
        announce(t('ai.resumedAnnounce'));
      })
      .catch((reason) => {
        if (generation === contextGeneration) setError(String(reason));
      });
  }

  // "Ask AI about selection" (docs/READER_ANNOTATIONS_PLAN.md §4.3) always
  // starts a new conversation with the selection inline, as a highlighted
  // `"quote"` reference the user can keep typing after.
  $effect(() => {
    if (!pendingQuote || sending) return;
    const selected = pendingQuote;
    startNewChat();
    if (item) context.reset([item], null, true);
    const label = quoteLabel(selected);
    draft = `${label} `;
    draftReferences = [{ kind: 'quote', text: label, full: selected }];
    queueMicrotask(() => {
      composerEl?.focus();
      composerEl?.setSelectionRange(draft.length, draft.length);
      resizeComposer();
    });
    onConsumedPendingQuote?.();
  });

  onMount(() => {
    composerEl?.focus();
    const unlisten = listen<AiStreamEvent>('ai-stream', (event) =>
      handleStreamEvent(event.payload),
    );
    void unlisten.catch((reason) => {
      if (!disposed) setError(String(reason));
    });
    return () => {
      disposed = true;
      contextGeneration++;
      loadGeneration++;
      clearTimeout(flushTimer);
      clearTimeout(copyTimer);
      flushTimer = undefined;
      streamBuffer = '';
      earlyEvents = [];
      transcript.dispose();
      run = null;
      void unlisten.then((stop) => stop()).catch(() => undefined);
    };
  });

  function scrollToEnd(force = false) {
    if (!messageList || (!force && !stickToBottom)) return;
    tick().then(() => {
      if (messageList) messageList.scrollTop = messageList.scrollHeight;
    });
  }

  function handleScroll() {
    if (!messageList) return;
    const { scrollTop, clientHeight, scrollHeight } = messageList;
    stickToBottom = scrollHeight - scrollTop - clientHeight < 48;
  }

  function flushStream() {
    clearTimeout(flushTimer);
    flushTimer = undefined;
    if (run) run.text = streamBuffer;
    scrollToEnd();
  }

  function scheduleFlush() {
    flushTimer ??= setTimeout(flushStream, resourceSettings().aiFlushIntervalMs);
  }

  function handleStreamEvent(event: AiStreamEvent) {
    if (disposed || !run) return;
    if (run.requestId === null) {
      const bytes = encoder.encode(event.text).byteLength;
      if (
        earlyOverflow ||
        earlyEvents.length >= 256 ||
        earlyEventBytes + bytes > 2 * MAX_STREAM_BYTES
      ) {
        earlyOverflow = true;
        earlyEvents = [];
        earlyEventBytes = 0;
      } else {
        earlyEvents.push(event);
        earlyEventBytes += bytes;
      }
      return;
    }
    if (event.requestId !== run.requestId) return;
    switch (event.kind) {
      case 'delta':
        streamBytes += encoder.encode(event.text).byteLength;
        if (streamBytes > MAX_STREAM_BYTES) {
          rejectOversizedStream();
          return;
        }
        streamBuffer += event.text;
        scheduleFlush();
        break;
      case 'replace':
        streamBytes = encoder.encode(event.text).byteLength;
        if (streamBytes > MAX_STREAM_BYTES) {
          rejectOversizedStream();
          return;
        }
        streamBuffer = event.text;
        scheduleFlush();
        break;
      case 'status':
        run.status = event.text;
        break;
      default:
        finishRun(event);
    }
  }

  function rejectOversizedStream() {
    const requestId = run?.requestId;
    clearTimeout(flushTimer);
    flushTimer = undefined;
    streamBuffer = '';
    earlyEvents = [];
    earlyEventBytes = 0;
    run = null;
    setError(t('ai.outputLimit'));
    if (requestId) void cancelAiMessage(requestId).catch(() => undefined);
  }

  async function finishRun(event: AiStreamEvent) {
    const finished = run;
    if (!finished) return;
    const finalText = event.kind === 'done' ? event.text || streamBuffer : streamBuffer;
    if (encoder.encode(finalText).byteLength > MAX_STREAM_BYTES) {
      rejectOversizedStream();
      return;
    }
    clearTimeout(flushTimer);
    flushTimer = undefined;
    run = null;
    streamBuffer = '';
    // The transcript belongs to the conversation, not to the open post, so it
    // is `loadGeneration` that says whether it is still wanted: only starting
    // or opening another conversation invalidates it.
    const generation = loadGeneration;
    selectedSessionId = event.sessionId;

    const localTurn: AiMessage[] = [
      localMessage('user', finished.prompt, event.sessionId),
      ...(finalText ? [localMessage('assistant', finalText, event.sessionId)] : []),
    ];

    if (event.kind === 'error') {
      errorKind = event.errorKind ?? null;
      error = event.text;
      transcript.appendLocal([localTurn[0]]);
    } else if (event.kind === 'cancelled') {
      // The CLI transcript won't have a stopped answer; keep what streamed.
      notice = t('ai.stopped');
      transcript.appendLocal(localTurn);
    } else {
      try {
        const page = await loadAiTranscript(event.sessionId);
        if (generation !== loadGeneration) return;
        if (page.messages.length > 0) transcript.accept(page);
        else {
          transcript.note = page.note;
          transcript.appendLocal(localTurn);
        }
      } catch {
        if (generation === loadGeneration) transcript.appendLocal(localTurn);
      }
    }
    scrollToEnd(true);
    // The history list below the composer is about the open post, so it is
    // that post's sessions that need refreshing, not the conversation's.
    const listPostId = item?.id;
    if (listPostId === undefined) return;
    const listGeneration = contextGeneration;
    try {
      const updated = await loadAllSessions(listPostId);
      if (listGeneration === contextGeneration) sessions = updated;
    } catch {
      // History refresh is best-effort; the conversation itself is shown.
    }
  }

  function localMessage(role: 'user' | 'assistant', content: string, sessionId: number): AiMessage {
    return {
      id: nextLocalId--,
      sessionId,
      role,
      content,
      createdAt: Math.floor(Date.now() / 1000),
    };
  }

  function startNewChat() {
    if (sending) return;
    // Cancels an in-flight transcript load (auto-resume, or a history click)
    // so it can't repopulate `messages` after the chat was emptied. That load
    // also owns `loadingMessages`, and its `finally` is now disarmed, so the
    // spinner has to be cleared here or it never goes away.
    loadGeneration += 1;
    loadingMessages = false;
    loadingPage = null;
    selectedSessionId = null;
    transcript.clear();
    error = null;
    notice = null;
    lastPrompt = '';
    transcript.note = null;
    historyOpen = false;
    resumedSession = null;
    draft = '';
    draftReferences = [];
    void tick().then(resizeComposer);
    // An empty chat has nothing to protect, so it follows the open post again.
    context.reset(standalone ? [] : item ? [item] : [], null, standalone);
    mention = null;
    dismissedSuggestions.clear();
  }

  /** Starts fresh for the open post; saved conversations remain in history. */
  function startChatForOpenPost() {
    if (sending) return;
    startNewChat();
    if (item) context.reset([item], null, true);
    composerEl?.focus();
  }

  /** The picker's onChange: an empty chat has no conversation to protect, so
   * it switches right away; otherwise it waits for confirmation (§D). */
  function requestProviderSwitch(next: AiProvider) {
    if (next === provider || sending || loadingMessages || updatingContext) return;
    if (selectedSessionId === null) selectProvider(next);
    else pendingProviderSwitch = next;
  }

  function confirmProviderSwitch() {
    if (pendingProviderSwitch) selectProvider(pendingProviderSwitch);
    pendingProviderSwitch = null;
  }

  function cancelProviderSwitch() {
    pendingProviderSwitch = null;
    providerPickerResetKey++;
  }

  function selectProvider(next: AiProvider) {
    if (provider === next || sending || loadingMessages || updatingContext) return;
    // A conversation belongs to one CLI, so switching starts a new one; the
    // old one stays in history and one click brings it back (§3.9).
    if (selectedSessionId !== null) {
      previousConversation = { provider, sessionId: selectedSessionId };
    }
    const keptItems = context.items;
    const keptScope = context.scopeLabel;
    const keptFilter = context.pendingScopeFilter;
    // The in-progress message belongs to the user, not to the conversation
    // being left behind, so it survives the switch too (§3.9).
    const keptDraft = draft;
    const keptReferences = draftReferences;
    provider = next;
    try {
      localStorage.setItem(PROVIDER_KEY, next);
    } catch {
      // The choice still applies for this session.
    }
    startNewChat();
    context.items = keptItems;
    context.scopeLabel = keptScope;
    context.pendingScopeFilter = keptFilter;
    context.pinned = keptItems.length > 1 || keptScope !== null;
    draft = keptDraft;
    draftReferences = keptReferences;
  }

  async function openSession(session: AiSession, resumed = false) {
    if (sending) return;
    const generation = ++loadGeneration;
    selectedSessionId = session.id;
    provider = session.provider;
    // Only an automatic resume earns the divider; picking a conversation from
    // the history popover is a deliberate act that needs no explaining.
    resumedSession = resumed ? session : null;
    // An opened conversation is worth keeping when the reader moves on. It is
    // about the post it was listed under, which is the one open right now.
    context.pinned = true;
    context.items =
      session.contextItems ??
      (item ? [{ itemId: item.id, itemTitle: item.title, feedId: item.feedId, position: 0 }] : []);
    context.scopeLabel = session.scopeLabel ?? null;
    context.pendingScopeFilter = null;
    previousConversation = null;
    historyOpen = false;
    loadingMessages = true;
    loadingPage = null;
    transcript.clear();
    transcript.note = null;
    error = null;
    notice = null;
    try {
      const result = await loadAiTranscript(session.id);
      if (generation === loadGeneration) {
        transcript.accept(result);
        scrollToEnd(true);
      }
    } catch (reason) {
      if (generation === loadGeneration) setError(String(reason));
    } finally {
      if (generation === loadGeneration) loadingMessages = false;
    }
  }

  async function loadTranscriptPage(direction: 'older' | 'newer' | 'latest') {
    if (selectedSessionId === null || sending || loadingMessages || loadingPage !== null) return;
    const generation = ++loadGeneration;
    const cursor =
      direction === 'older' && transcript.before
        ? { before: transcript.before }
        : direction === 'newer' && transcript.after
          ? { after: transcript.after }
          : {};
    loadingPage = direction;
    error = null;
    try {
      const page = await loadAiTranscript(selectedSessionId, cursor);
      if (generation !== loadGeneration) return;
      transcript.accept(page);
      await tick();
      if (generation === loadGeneration && messageList) {
        stickToBottom = direction === 'latest';
        messageList.scrollTop = direction === 'latest' ? messageList.scrollHeight : 0;
      }
    } catch (reason) {
      if (generation === loadGeneration) setError(String(reason));
    } finally {
      if (generation === loadGeneration) loadingPage = null;
    }
  }

  function goBackToPrevious() {
    const target = [...sessions, ...allSessions].find(
      (session) => session.id === previousConversation?.sessionId,
    );
    if (target) openSession(target);
  }

  async function removeSession(session: AiSession) {
    if (sending) return;
    try {
      await deleteAiSession(session.id);
      allSessions = allSessions.filter((entry) => entry.id !== session.id);
      sessions = sessions.filter((entry) => entry.id !== session.id);
      onHistoryChange?.();
      if (selectedSessionId === session.id) startNewChat();
      confirmingRemoveId = null;
      if (item) sessions = await loadAllSessions(item.id);
      announce(t('ai.removedAnnounce'));
    } catch (reason) {
      setError(String(reason));
    }
  }

  function askChip(question: string) {
    submitMessage(question);
  }

  /** Matches `currentFilter()`'s "Today" bounds in +page.svelte. */
  function todayFilter(): ItemFilter {
    const now = new Date();
    return {
      publishedFrom: Math.floor(
        new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000,
      ),
      publishedTo: Math.floor(
        new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime() / 1000,
      ),
    };
  }

  /**
   * Standalone-only quick actions: re-runs a service-wide `ItemFilter` (the
   * same pattern as the sidebar's "Summarize view") directly from the empty
   * panel, with no feed/folder/article already open.
   */
  async function askScope(filter: ItemFilter, scopeLabel: string, prompt: string) {
    if (sending || loadingMessages || updatingContext || disposed) return;
    try {
      const found = await listItems(filter, 20);
      if (disposed) return;
      if (found.length === 0) {
        notice = t('ai.scopeEmpty');
        return;
      }
      context.reset(found, scopeLabel, true, filter);
      notice = null;
      void askChip(prompt);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function addItems(items: Item[]): Promise<boolean> {
    if (sending || loadingMessages || updatingContext || disposed) return false;
    const generation = loadGeneration;
    const sessionId = selectedSessionId;
    updatingContext = true;
    try {
      if (sessionId !== null) {
        const session = await addAiContextItems(
          sessionId,
          items.map((i) => i.id),
        );
        if (generation !== loadGeneration || disposed) return false;
        context.items = session.contextItems ?? context.items;
      } else context.add(items);
      if (sessionId !== null) onHistoryChange?.();
      announce(t('ai.contextAdded'));
      return true;
    } catch (reason) {
      if (generation === loadGeneration) setError(String(reason));
      return false;
    } finally {
      updatingContext = false;
    }
  }

  async function removeContext(position: number) {
    if (sending || loadingMessages || updatingContext) return;
    const generation = loadGeneration;
    const sessionId = selectedSessionId;
    const itemId = context.items.find((i) => i.position === position)?.itemId;
    updatingContext = true;
    try {
      if (sessionId !== null) {
        const session = await removeAiContextItem(sessionId, position);
        if (generation !== loadGeneration) return;
        context.items = session.contextItems ?? [];
      } else
        context.items = context.items
          .filter((i) => i.position !== position)
          .map((i, position) => ({ ...i, position }));
      context.pinned = true;
      if (sessionId !== null) onHistoryChange?.();
      context.pendingScopeFilter = null;
      announce(t('ai.contextRemoved'));
      // Keeps the composer from showing an "@" reference for an article the
      // chips popover just removed - the other half of reconcileDraftReferences.
      const tracked = draftReferences.find((r) => r.kind === 'article' && r.itemId === itemId);
      if (tracked) {
        draft = draft.replace(tracked.text, '').trimStart();
        draftReferences = draftReferences.filter((m) => m !== tracked);
        void tick().then(resizeComposer);
      }
    } catch (reason) {
      if (generation === loadGeneration) setError(String(reason));
    } finally {
      updatingContext = false;
    }
  }

  let lastContextRequest = 0;
  $effect(() => {
    const request = contextRequest;
    if (!request || request.key === lastContextRequest || sending || updatingContext) return;
    lastContextRequest = request.key;
    untrack(() => {
      if (request.add) void addItems(request.items);
      else {
        startNewChat();
        context.reset(request.items, request.scopeLabel ?? null, true, request.scopeFilter ?? null);
        if (request.action === 'summarize')
          void askChip('Summarize this post: the main idea and the key points.');
        else queueMicrotask(() => composerEl?.focus());
      }
    });
  });

  async function toggleHistory() {
    historyOpen = !historyOpen;
    if (historyOpen) {
      if (!item || standalone) historyTab = 'all';
      try {
        allSessions = await listAllAiSessions();
      } catch (reason) {
        setError(String(reason));
      }
    }
  }

  async function submitMessage(text = draft.trim()) {
    // The question goes to the post this conversation is about, not whatever
    // the reader happens to be showing.
    if (sending || loadingMessages || loadingPage !== null || updatingContext || disposed) return;
    if (/(^|\s)@this(?=\s|$)/i.test(text)) {
      if (!item) {
        notice = t('ai.mention.noCurrent');
        return;
      }
      const target = item;
      if (!context.contains(target.id) && !(await addItems([target]))) return;
      context.pinned = true;
      notice = null;
      mention = null;
    }
    if (mention !== null) {
      notice = t('ai.mention.chooseFirst');
      return;
    }
    if (!text) return;
    const prompt = resolveQuoteReferences(text);

    const generation = ++loadGeneration;
    context.pinned = true;
    run = { requestId: null, prompt, text: '', status: null };
    streamBuffer = '';
    streamBytes = 0;
    earlyEvents = [];
    earlyEventBytes = 0;
    earlyOverflow = false;
    draft = '';
    draftReferences = [];
    error = null;
    notice = null;
    lastPrompt = prompt;
    previousConversation = null;
    resumedSession = null;
    stickToBottom = true;
    scrollToEnd(true);
    if (composerEl) composerEl.style.height = '';

    try {
      if (selectedSessionId === null && context.pendingScopeFilter !== null) {
        const current = await listItems(context.pendingScopeFilter, 20);
        if (disposed || generation !== loadGeneration || !run) return;
        if (current.length === 0) throw new Error(t('ai.scopeEmpty'));
        // This is the send-time snapshot; later turns never re-query the filter.
        context.reset(current, context.scopeLabel, true);
      }
      const started = await startAiMessage(
        context.ids,
        provider,
        selectedSessionId,
        prompt,
        context.scopeLabel,
      );
      // Only a `startNewChat`/`stopRun` in between clears `run`, and either
      // way this answer is no longer the one on screen.
      if (disposed || generation !== loadGeneration || !run) return;
      run.requestId = started.requestId;
      selectedSessionId = started.sessionId;
      onHistoryChange?.();
      if (earlyOverflow) {
        rejectOversizedStream();
        return;
      }
      const queued = earlyEvents;
      earlyEvents = [];
      earlyEventBytes = 0;
      for (const event of queued) handleStreamEvent(event);
    } catch (reason) {
      if (!disposed && generation === loadGeneration) {
        run = null;
        setError(String(reason));
      }
    }
  }

  function stopRun() {
    if (run?.requestId) cancelAiMessage(run.requestId);
  }

  // True right after a stop, until the next send (submitMessage clears
  // `notice`) or a fresh stop overwrites it - drives the "continue" button.
  const canContinueAfterStop = $derived(!sending && notice !== null && notice === t('ai.stopped'));

  /** The CLI process is killed on stop, so there is no mid-answer to resume -
   * this sends a new turn asking the assistant to pick back up, over the same
   * (already `--resume`-able) session. */
  function continueAfterStop() {
    submitMessage(t('ai.continuePrompt'));
  }

  /** Re-asks the user turn right before the given assistant message (only
   * ever shown for the latest turn - see the `messageIndex` check in the
   * template - so this never re-plays a mid-history message against the
   * CLI's linear `--resume` session). */
  function regenerate(assistantIndex: number) {
    const precedingUser = messages[assistantIndex - 1];
    if (precedingUser?.role === 'user') submitMessage(precedingUser.content);
  }

  function resizeComposer() {
    if (!composerEl) return;
    composerEl.style.height = 'auto';
    composerEl.style.height = `${Math.min(composerEl.scrollHeight, 180)}px`;
    syncComposerScroll();
  }

  function observeComposer(node: HTMLTextAreaElement) {
    let width = -1;
    let frame = 0;
    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(([entry]) => {
            if (entry.contentRect.width === width) return;
            width = entry.contentRect.width;
            cancelAnimationFrame(frame);
            if (width > 0) frame = requestAnimationFrame(resizeComposer);
          });
    observer?.observe(node);
    return {
      destroy() {
        observer?.disconnect();
        cancelAnimationFrame(frame);
      },
    };
  }

  async function copyMessage(message: AiMessage) {
    try {
      await navigator.clipboard.writeText(message.content);
      copiedMessageId = message.id;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        if (copiedMessageId === message.id) copiedMessageId = null;
      }, 1400);
    } catch {
      setError(t('ai.copyFailed'));
    }
  }

  /**
   * One polite region for conversation-level news (resumed a chat, added a
   * post to the context). The streaming answer is already a live region, so
   * this one stays quiet while a run is in flight rather than interleaving
   * with it, and self-clears so the same text announces again next time.
   */
  function announce(text: string) {
    if (sending) return;
    liveMessage = text;
    window.setTimeout(() => {
      if (liveMessage === text) liveMessage = '';
    }, 1000);
  }
</script>

{#snippet quickAction(label: MessageKey, description: MessageKey, prompt: string)}
  <button
    type="button"
    class="quick-action"
    aria-label={t(label)}
    title={t(description)}
    onclick={() => askChip(prompt)}
  >
    <span>{t(label)}</span><Icon name="chevron-right" size={13} />
  </button>
{/snippet}

{#snippet scopeAction(
  label: MessageKey,
  description: MessageKey,
  filter: ItemFilter,
  scopeLabel: string,
  prompt: string,
)}
  <button
    type="button"
    class="quick-action"
    aria-label={t(label)}
    title={t(description)}
    onclick={() => askScope(filter, scopeLabel, prompt)}
  >
    <span>{t(label)}</span><Icon name="chevron-right" size={13} />
  </button>
{/snippet}

<aside class="ai-panel" aria-label={t(standalone ? 'ai.chatTitle' : 'ai.panel')}>
  <p class="visually-hidden" role="status">{liveMessage}</p>
  <header class="panel-header">
    <div class="panel-identity">
      <Icon name="ask-ai" size={15} />
      <div class="panel-titles">
        <h2>{t(standalone ? 'ai.chatTitle' : 'ai.panel')}</h2>
        <p class="panel-subtitle">{t(standalone ? 'ai.chatHint' : 'ai.panelHint')}</p>
      </div>
    </div>
    <div class="header-actions">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="history-anchor"
        use:clickOutside={() => (historyOpen = false)}
        onkeydown={(event) => {
          if (event.key === 'Escape' && historyOpen) {
            event.stopPropagation();
            historyOpen = false;
          }
        }}
      >
        <IconButton
          icon="clock"
          label={t('ai.history')}
          class="text-action"
          ariaHaspopup="dialog"
          ariaExpanded={historyOpen}
          hasDot={sessions.length > 0}
          onclick={toggleHistory}
        />
        {#if historyOpen}
          <div
            class="history-popover ui-popup"
            role="dialog"
            aria-label={t('ai.history')}
            tabindex="-1"
            onkeydown={(event) => {
              if (event.key === 'Escape') {
                event.stopPropagation();
                historyOpen = false;
                (
                  event.currentTarget.parentElement?.querySelector(
                    '.text-action',
                  ) as HTMLButtonElement
                )?.focus();
              }
            }}
          >
            <button
              type="button"
              class="ui-option new-chat-option"
              disabled={sending || isEmptyChat}
              onclick={startNewChat}><Icon name="plus" size={13} />{t('ai.newChat')}</button
            >
            <div class="history-tabs">
              <SegmentedControl
                value={historyTab}
                ariaLabel={t('ai.historyFilter')}
                size="sm"
                options={item
                  ? [
                      { value: 'item' as const, label: t('ai.historyItem') },
                      { value: 'all' as const, label: t('ai.historyAll') },
                    ]
                  : [{ value: 'all' as const, label: t('ai.historyAll') }]}
                onChange={(value) => (historyTab = value)}
              />
            </div>
            {#if visibleSessions.length === 0}
              <p class="history-empty">{t('ai.historyEmpty')}</p>
            {:else}
              <ul>
                {#each visibleSessions as session (session.id)}
                  <li
                    class="ui-list-row"
                    class:current={selectedSessionId === session.id}
                    class:ui-selected={selectedSessionId === session.id}
                  >
                    {#if confirmingRemoveId === session.id}
                      <div class="confirm-row">
                        <span class="confirm-note">{t('ai.historyNote')}</span>
                        <div class="confirm-actions">
                          <button
                            type="button"
                            class="ui-action"
                            onclick={() => (confirmingRemoveId = null)}>{t('common.cancel')}</button
                          >
                          <button
                            type="button"
                            class="confirm-remove ui-action"
                            onclick={() => removeSession(session)}>{t('ai.remove')}</button
                          >
                        </div>
                      </div>
                    {:else}
                      <button
                        class="history-open ui-option"
                        type="button"
                        title={session.title}
                        aria-current={selectedSessionId === session.id ? 'true' : undefined}
                        disabled={sending}
                        onclick={() => openSession(session)}
                      >
                        <span class="history-title">{session.title}</span>
                        <span class="history-meta">
                          <Icon name={PROVIDER_ICONS[session.provider]} size={11} />
                          {PROVIDER_NAMES[session.provider]} · {formatRelative(session.updatedAt)}
                        </span>
                      </button>
                      <IconButton
                        icon="close"
                        class="history-remove"
                        size="sm"
                        iconSize={12}
                        label={t('ai.removeConversation')}
                        disabled={sending}
                        onclick={() => (confirmingRemoveId = session.id)}
                      />
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {/if}
      </div>
      <AiProviderPicker
        value={provider}
        label={t('ai.providerLabel')}
        hint={sending ? t('ai.providerBusy') : selectedSessionId ? t('ai.modelHint') : undefined}
        disabled={sending || loadingMessages || updatingContext || pendingProviderSwitch !== null}
        compact
        onChange={requestProviderSwitch}
        resetKey={providerPickerResetKey}
      />
      <IconButton icon="close" label={t('ai.close')} onclick={onCollapse} />
    </div>
  </header>
  {#if pendingProviderSwitch}
    <div class="confirm-row provider-switch-confirm">
      <span class="confirm-note">{t('ai.switchProviderConfirm')}</span>
      <div class="confirm-actions">
        <button type="button" class="ui-action" onclick={cancelProviderSwitch}
          >{t('common.cancel')}</button
        >
        <button type="button" class="ui-action" onclick={confirmProviderSwitch}
          >{t('ai.switchProviderAction')}</button
        >
      </div>
    </div>
  {/if}
  {#if !standalone}
    <AiContextChips
      items={context.items}
      scopeLabel={context.scopeLabel}
      {feeds}
      activeItemId={item?.id ?? null}
      disabled={sending || loadingMessages || updatingContext}
      onOpen={(id) => onOpenItem?.(id)}
      onRemove={removeContext}
    />
  {/if}
  {#if context.scopeLabel}<p class="subtle">{t('ai.summaryBasis')}</p>{/if}

  <div class="chat-messages" use:sourceLinks bind:this={messageList} onscroll={handleScroll}>
    {#if previousConversation}
      <p class="switch-note">
        <Icon name={PROVIDER_ICONS[previousConversation.provider]} size={11} />
        {t('ai.switchedProvider', { provider: PROVIDER_NAMES[previousConversation.provider] })}
        · <button type="button" onclick={goBackToPrevious}>{t('ai.backToConversation')}</button>
      </p>
    {/if}

    {#if isEmptyChat}
      <div class="chat-empty">
        <div class="welcome">
          <h3>
            {t(
              context.items.length === 0
                ? 'ai.welcomeGeneral'
                : context.items.length > 1
                  ? 'ai.welcomeMultiple'
                  : 'ai.welcome',
            )}
          </h3>
          {#if !context.items.length}<p>
              {t(standalone ? 'ai.mention.welcomeHint' : 'ai.noContextHint')}
            </p>{/if}
        </div>
        {#if context.items.length}
          <div class="quick-actions">
            {#if context.scopeLabel}
              {@render quickAction(
                'ai.chip.topThree',
                'ai.action.topThree',
                'Give the three key ideas in these articles.',
              )}
              {@render quickAction(
                'ai.chip.group',
                'ai.action.group',
                'Group these articles by topic.',
              )}
              {@render quickAction(
                'ai.chip.choose',
                'ai.action.choose',
                'Which articles are worth reading and why?',
              )}
            {:else if context.items.length > 1}
              {@render quickAction(
                'ai.chip.compare',
                'ai.action.compare',
                'Compare these articles.',
              )}
              {@render quickAction(
                'ai.chip.common',
                'ai.action.common',
                'What do these articles have in common?',
              )}
              {@render quickAction(
                'ai.chip.takeaways',
                'ai.action.summary',
                'Summarize only the key points of these articles.',
              )}
              {@render quickAction(
                'ai.chip.differing',
                'ai.action.issues',
                'Find differing claims in these articles.',
              )}
            {:else}
              {@render quickAction(
                'ai.chip.summarize',
                'ai.action.summary',
                'Summarize this post: the main idea and the key points.',
              )}
              {@render quickAction(
                'ai.chip.explain',
                'ai.action.explain',
                'Explain this post in simple terms.',
              )}
              {@render quickAction(
                'ai.chip.issues',
                'ai.action.issues',
                'What are the key issues discussed in this article?',
              )}
            {/if}
          </div>
        {:else if standalone}
          <div class="quick-actions">
            {@render scopeAction(
              'ai.chip.today',
              'ai.action.today',
              todayFilter(),
              `◷ ${t('posts.label.today')}`,
              'Summarize the articles published today.',
            )}
            {@render scopeAction(
              'ai.chip.unread',
              'ai.action.unread',
              { unreadOnly: true },
              t('posts.label.unread'),
              'Of my unread articles, which ones matter most, and why?',
            )}
            {@render scopeAction(
              'ai.chip.starred',
              'ai.action.starred',
              { savedOnly: true },
              `★ ${t('posts.label.starred')}`,
              'Compare the articles I have starred.',
            )}
          </div>
        {:else}
          <div class="quick-actions">
            <button
              type="button"
              class="quick-action"
              aria-label={t('ai.chip.mention')}
              title={t('ai.action.mentionHint')}
              onclick={startMentionHint}
            >
              <span>{t('ai.chip.mention')}</span><Icon name="chevron-right" size={13} />
            </button>
          </div>
        {/if}
      </div>
    {:else if loadingMessages}
      <p class="subtle">{t('ai.loading')}</p>
    {:else if selectedSessionId && messages.length === 0 && !sending}
      <p class="subtle" role="status">{transcriptNote ?? t('ai.transcriptMissing')}</p>
    {/if}

    {#if resumedSession && messages.length > 0 && !sending}
      <p class="resumed-note">
        <span>{t('ai.resumed', { when: formatRelative(resumedSession.updatedAt) })}</span>
        · <button type="button" onclick={startNewChat}>{t('ai.resumedNew')}</button>
      </p>
    {/if}

    {#if transcript.hasOlder && !sending}
      <button
        type="button"
        class="transcript-page"
        disabled={loadingPage !== null}
        onclick={() => loadTranscriptPage('older')}
      >
        {loadingPage === 'older'
          ? t('ai.loadingPage')
          : transcript.before
            ? t('ai.olderMessages')
            : t('ai.latestMessages')}
      </button>
    {/if}
    {#if transcript.budgetLimited}
      <p class="subtle" role="status">{t('ai.cacheBudget')}</p>
    {/if}

    {#each messages as message, messageIndex (message.id)}
      {#if message.role === 'user'}
        <div class="message user">
          <span class="visually-hidden">{t('ai.you')}</span>
          <div class="message-content">
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            {@html renderMarkdown(message.content, new Map(), { mentionLabels })}
          </div>
        </div>
      {:else}
        <div class="message assistant">
          <span class="visually-hidden">{PROVIDER_NAMES[provider]}</span>
          <div class="message-content">
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            {@html renderMarkdown(message.content, citations, {
              sourceLabel: t('ai.source'),
              includeSources: true,
              fallbackSources: [...citations.values()],
            })}
          </div>
          <IconButton
            class="copy"
            size="sm"
            icon={copiedMessageId === message.id ? 'check' : 'copy'}
            label={copiedMessageId === message.id ? t('ai.copied') : t('ai.copy')}
            onclick={() => copyMessage(message)}
          />
          {#if messageIndex === messages.length - 1 && !transcript.hasNewer && !sending}
            <IconButton
              class="regenerate"
              size="sm"
              icon="retry"
              label={t('ai.regenerate')}
              onclick={() => regenerate(messageIndex)}
            />
          {/if}
        </div>
      {/if}
    {/each}

    {#if transcript.hasNewer && !sending}
      <button
        type="button"
        class="transcript-page"
        disabled={loadingPage !== null}
        onclick={() => loadTranscriptPage('newer')}
        >{loadingPage === 'newer' ? t('ai.loadingPage') : t('ai.newerMessages')}</button
      >
      <button
        type="button"
        class="transcript-page"
        disabled={loadingPage !== null}
        onclick={() => loadTranscriptPage('latest')}
        >{loadingPage === 'latest' ? t('ai.loadingPage') : t('ai.latestMessages')}</button
      >
    {/if}

    {#if run && sending}
      <div class="message user">
        <div class="message-content">
          <!-- eslint-disable-next-line svelte/no-at-html-tags -->
          {@html renderMarkdown(run.prompt, new Map(), { mentionLabels })}
        </div>
      </div>
      <div class="message assistant" aria-live="polite" aria-busy="true">
        {#if run.text}
          <div class="message-content">
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            {@html renderMarkdown(run.text, citations, { sourceLabel: t('ai.source') })}
          </div>
        {:else}
          <ThinkingDots
            label={t(
              run.status === 'thinking'
                ? 'ai.status.thinking'
                : run.status === 'tool'
                  ? 'ai.status.tool'
                  : 'ai.status.working',
            )}
          />
        {/if}
      </div>
    {/if}

    {#if transcriptNote && selectedSessionId && messages.length > 0}
      <p class="subtle" role="status">{transcriptNote}</p>
    {/if}
    {#if notice}
      <p class="subtle" role="status">{notice}</p>
      {#if canContinueAfterStop}
        <button type="button" class="transcript-page" onclick={continueAfterStop}>
          {t('ai.continue')}
        </button>
        <p class="subtle continue-hint">{t('ai.continueHint')}</p>
      {/if}
    {/if}
    {#if updatingContext}
      <p class="subtle" role="status">{t('ai.contextUpdating')}</p>
    {/if}
  </div>

  {#if error}
    <ErrorCard
      title={errorTitle}
      body={error}
      onRetry={lastPrompt && !sending ? () => submitMessage(lastPrompt) : undefined}
      retryLabel={t('ai.retry')}
      onDismiss={() => (error = null)}
      dismissLabel={t('ai.dismissError')}
    />
  {/if}

  <form
    class="composer"
    onsubmit={(event) => {
      event.preventDefault();
      submitMessage();
    }}
  >
    <div class="composer-anchor" use:clickOutside={() => (mention = null)}>
      {#if mention && !sending && !loadingMessages && !updatingContext}
        <AiArticleMentions
          bind:this={mentionPicker}
          id={mentionListId}
          query={mention.query}
          currentItem={item}
          selectedIds={context.ids}
          {feeds}
          atLimit={context.items.length >= 20}
          onSelect={chooseMention}
          onClose={() => {
            mention = null;
            composerEl?.focus();
          }}
          onActiveChange={(id) => (activeMentionId = id)}
        />
      {/if}
      <div class="composer-surface">
        {#if standalone && context.items.length}
          <AiContextChips
            placement="above"
            items={context.items}
            scopeLabel={context.scopeLabel}
            {feeds}
            activeItemId={item?.id ?? null}
            disabled={sending || loadingMessages || updatingContext}
            onOpen={(id) => onOpenItem?.(id)}
            onRemove={removeContext}
          />
        {/if}
        {#if otherPostOpen && item}
          <AiSuggestionBar
            title={item.title ?? '(untitled)'}
            onAdd={!sending && !loadingMessages && context.items.length < 20
              ? () => addItems([item!])
              : undefined}
            disabled={sending || loadingMessages || updatingContext}
            onStartHere={startChatForOpenPost}
            onDismiss={() => dismissedSuggestions.add(item.id)}
          />
        {/if}
        <div class="composer-row">
          <div class="composer-text">
            <!-- Mirrors the textarea's text so `@` references can be
                 highlighted - a plain textarea can't style part of its own
                 value. Not interactive; the real textarea above it owns
                 focus, caret and selection. -->
            <div class="composer-backdrop" aria-hidden="true" bind:this={composerBackdropEl}>
              <!-- eslint-disable-next-line svelte/no-at-html-tags -->
              {@html highlightedComposerHtml(draft, draftReferences)}
            </div>
            <textarea
              bind:value={draft}
              bind:this={composerEl}
              use:observeComposer
              rows="1"
              aria-label={t('ai.composer')}
              title={t('ai.inputHint')}
              placeholder={t('ai.mention.placeholder')}
              aria-autocomplete="list"
              aria-controls={mention ? mentionListId : undefined}
              aria-activedescendant={mention ? activeMentionId : undefined}
              oninput={() => {
                resizeComposer();
                syncMention();
                void reconcileDraftReferences();
              }}
              onscroll={syncComposerScroll}
              onclick={syncMention}
              onselect={syncMention}
              onblur={(event) => {
                if (!(event.relatedTarget as Element | null)?.closest('.mention-picker'))
                  mention = null;
              }}
              onkeyup={(event) => {
                if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) syncMention();
              }}
              onkeydown={(event) => {
                if (mention && mentionPicker?.handleKeydown(event)) return;
                // Enter sends, Shift+Enter breaks the line; never while an IME
                // (e.g. Korean) is still composing a character.
                if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
                  event.preventDefault();
                  submitMessage();
                }
              }}></textarea>
          </div>
          {#if sending}
            <button
              class="send stop"
              type="button"
              aria-label={t('ai.stop')}
              title={t('ai.stop')}
              disabled={!run?.requestId}
              onclick={stopRun}
              ><Icon name="stop" size={14} /><span>{t('ai.stopShort')}</span></button
            >
          {:else}
            <button
              class="send"
              type="submit"
              aria-label={t('ai.send')}
              title={t('ai.send')}
              disabled={!draft.trim() || updatingContext || loadingMessages}
              ><Icon name="send" size={14} /></button
            >
          {/if}
        </div>
      </div>
    </div>
    {#if sending}<p class="composer-status" role="status">{t('ai.status.writing')}</p>
    {/if}
  </form>
</aside>

<style>
  .ai-panel {
    display: flex;
    min-width: 0;
    height: 100%;
    flex-direction: column;
    background: var(--color-bg);
    color: var(--color-text);
  }

  .panel-header {
    position: relative;
    display: flex;
    min-height: 48px;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  .history-anchor {
    position: relative;
  }

  .history-popover {
    position: absolute;
    top: calc(100% + var(--space-1));
    right: 0;
    z-index: 30;
    width: min(320px, calc(100vw - 2 * var(--space-3)));
    max-height: 360px;
    overflow-y: auto;
  }

  .new-chat-option {
    flex-direction: row;
    align-items: center;
    gap: var(--space-1);
    color: var(--color-text);
  }

  .history-popover ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .history-tabs {
    position: sticky;
    top: calc(-1 * var(--space-2));
    z-index: 1;
    padding: 0 0 var(--space-2);
    background: var(--color-bg);
  }
  .history-popover li :global(.history-remove) {
    flex-shrink: 0;
    opacity: 0;
    pointer-events: none;
  }
  .history-popover li:hover :global(.history-remove),
  .history-popover li:focus-within :global(.history-remove) {
    opacity: 1;
    pointer-events: auto;
  }
  @media (hover: none) {
    .history-popover li :global(.history-remove) {
      opacity: 1;
      pointer-events: auto;
    }
  }

  .history-popover .confirm-row {
    flex: 1;
  }

  .history-open {
    flex: 1;
  }

  .history-open:hover:not(:disabled) {
    background: var(--color-hover-bg);
  }

  .history-title {
    display: -webkit-box;
    overflow: hidden;
    font-size: var(--text-small);
    line-height: 1.5;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow-wrap: anywhere;
  }

  .history-meta {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }

  .history-meta :global(svg) {
    flex-shrink: 0;
  }

  .history-empty {
    margin: var(--space-3);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .transcript-page {
    align-self: center;
    padding: 6px var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .chat-messages {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: var(--space-4);
    overflow-y: auto;
    padding: var(--space-4) var(--space-4) var(--space-3);
  }

  .switch-note {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .switch-note :global(svg) {
    /* Icon baseline nudge, not layout rhythm - left raw on purpose. */
    margin-right: 2px;
    vertical-align: -1px;
  }

  .switch-note button {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-accent);
    font: inherit;
    cursor: pointer;
  }

  /* Marks where the conversation this panel reopened begins, so continuing
     an old thread never looks like a fresh one. A hairline rule either side
     keeps it readable as a separator rather than another message. */
  .resumed-note {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
  }

  .resumed-note::before,
  .resumed-note::after {
    flex: 1;
    border-top: 1px solid var(--color-border);
    content: '';
  }

  .resumed-note::before {
    margin-right: var(--space-2);
  }

  .resumed-note::after {
    margin-left: var(--space-2);
  }

  .resumed-note button {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-accent);
    font: inherit;
    cursor: pointer;
  }

  .chat-empty {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    margin-top: var(--space-2);
  }

  .subtle {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }

  .continue-hint {
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }

  .message {
    position: relative;
    min-width: 0;
  }

  .message.user {
    align-self: flex-end;
    max-width: 85%;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
    background: var(--color-hover-bg);
  }

  .message.assistant {
    padding-right: var(--space-5);
  }

  /* The copy button is IconButton's own root element, which carries this
     repo's usual Svelte scoping class, not AiPanel's - :global() matches it
     by plain class name instead of relying on that scoping. */
  .message :global(.copy) {
    position: absolute;
    top: calc(-1 * var(--space-1));
    right: 0;
    opacity: 0;
  }

  .message.assistant:hover :global(.copy),
  .message :global(.copy:focus-visible) {
    opacity: 1;
  }

  .message-content {
    font-size: var(--text-body);
    line-height: 1.6;
    overflow-wrap: anywhere;
    word-break: keep-all;
  }

  .message-content :global(p),
  .message-content :global(ul),
  .message-content :global(ol),
  .message-content :global(pre),
  .message-content :global(blockquote),
  .message-content :global(.table-scroll) {
    margin: 0 0 var(--space-2);
  }

  .message-content :global(:last-child) {
    margin-bottom: 0;
  }

  .message-content :global(h1),
  .message-content :global(h2),
  .message-content :global(h3) {
    margin: var(--space-3) 0 var(--space-1);
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
  }

  .message-content :global(h4) {
    margin: var(--space-2) 0 var(--space-1);
    font-size: var(--text-body);
    font-weight: var(--weight-bold);
  }

  .message-content :global(ul),
  .message-content :global(ol) {
    padding-left: 1.3em;
  }

  .message-content :global(li) {
    /* AI-rendered prose rhythm, not generic layout spacing - rounding to a
       token would double list-item spacing across every assistant answer. */
    margin: 2px 0;
  }

  .message-content :global(code) {
    padding: 1px var(--space-1);
    border-radius: var(--radius-xs);
    background: var(--color-bg-secondary);
    font-size: var(--text-small);
  }

  .message-content :global(pre) {
    overflow-x: auto;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    background: var(--color-bg-secondary);
  }

  .message-content :global(pre code) {
    padding: 0;
    background: transparent;
    font-size: var(--text-small);
  }

  .message-content :global(blockquote) {
    border-left: 2px solid var(--color-border);
    padding-left: var(--space-2);
    color: var(--color-text-secondary);
  }

  .message-content :global(blockquote p) {
    margin: 0 0 var(--space-1);
  }

  .message-content :global(.table-scroll) {
    overflow-x: auto;
  }

  .message-content :global(table) {
    border-collapse: collapse;
    width: 100%;
    font-size: var(--text-small);
  }

  .message-content :global(th),
  .message-content :global(td) {
    border: 1px solid var(--color-border);
    padding: var(--space-1) var(--space-2);
    text-align: left;
  }

  .message-content :global(th) {
    background: var(--color-bg-secondary);
    font-weight: var(--weight-bold);
    white-space: nowrap;
  }

  .message-content :global(a) {
    color: var(--color-accent);
  }

  /* ErrorCard's own root element now carries this class, not something
     rendered directly here, so :global() is needed to reach past its
     Svelte scoping - scoped under .ai-panel to stay local to this panel. */
  .ai-panel :global(.error-card) {
    margin: 0 var(--space-3) var(--space-2);
  }

  .composer {
    border-top: 1px solid var(--color-border);
    padding: var(--space-3);
    background: var(--color-bg);
  }

  .composer-surface {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    overflow: visible;
  }
  .composer-row {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-1) var(--space-1) var(--space-3);
    border: 0;

    background: transparent;
    border-radius: inherit;
  }

  .composer-surface:focus-within {
    border-color: var(--color-accent);
  }

  .composer-text {
    /* Establishes its own stacking context (position + explicit z-index) so
       the backdrop's z-index:-1 below is scoped to just these two children,
       not to some distant ancestor - otherwise the backdrop (and its text)
       paints behind the opaque .composer-surface background and vanishes. */
    position: relative;
    z-index: 0;
    min-width: 0;
    /* Guarantees the single-line case matches the send button's height
       exactly (both driven by the same token), instead of leaving it to
       match the textarea's independently-computed line-height box, which
       can be a subpixel off and read as visible misalignment. */
    min-height: var(--control-size);
    flex: 1;
  }

  /* Mirrors the textarea's text underneath it so `@` references can be
     highlighted - every sizing-relevant property here must match the
     textarea's exactly, or the two would wrap at different points and the
     visible (backdrop) text would drift from the real (invisible) caret. */
  .composer-backdrop {
    position: absolute;
    inset: 0;
    z-index: -1;
    overflow: hidden;
    padding: 6px 0;
    font: inherit;
    font-size: var(--text-body);
    line-height: 1.5;
    color: var(--color-text);
    white-space: pre-wrap;
    overflow-wrap: break-word;
    pointer-events: none;
  }

  .composer-backdrop :global(.mention-highlight) {
    /* Padding gives it room to read as a badge instead of a bare color wash
       hugging the glyphs; the equal negative margin cancels the extra width
       it would otherwise add, so the backdrop still wraps at the exact same
       point as the real (invisible) textarea text underneath it. */
    margin: -1px -3px;
    padding: 1px 3px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--color-accent) 16%, transparent);
    color: var(--color-accent);
  }

  textarea {
    width: 100%;
    max-height: 180px;
    padding: 6px 0;
    border: 0;
    background: transparent;
    color: transparent;
    caret-color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    line-height: 1.5;
    resize: none;
  }

  textarea:focus {
    outline: none;
  }

  .send {
    display: grid;
    width: var(--control-size);
    height: var(--control-size);
    flex-shrink: 0;
    place-items: center;
    border: 0;
    border-radius: var(--control-radius);
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
    cursor: pointer;
  }

  .send:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .send.stop {
    display: flex;
    width: auto;
    gap: var(--space-1);
    padding: 0 var(--space-2);
    font: inherit;
    font-size: var(--text-small);
    background: var(--color-text);
    color: var(--color-bg);
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .panel-header h2 {
    margin: 0;
    font-size: var(--text-body);
    font-weight: var(--weight-bold);
  }
  .panel-titles {
    display: flex;
    min-width: 0;
    flex-direction: column;
    /* Hairline between title/subtitle, not a spacing-rhythm value. */
    gap: 1px;
  }
  .panel-subtitle {
    overflow: hidden;
    margin: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .quick-action:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .welcome h3 {
    margin: 0 0 var(--space-2);
    font-size: var(--text-heading);
    font-weight: var(--weight-bold);
    line-height: 1.5;
  }
  .welcome p {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.6;
  }
  .composer-status {
    margin: var(--space-2) 0 0;
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
  }

  .panel-identity {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .header-actions {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
  }
  .quick-actions {
    display: grid;
    gap: var(--space-1);
  }
  .quick-action {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: 36px;
    width: 100%;
    padding: var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    background: transparent;
    font: inherit;
    font-size: var(--text-body);
    text-align: left;
    cursor: pointer;
  }
  .quick-action:hover {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }
  .quick-action :global(svg) {
    opacity: 0;
  }
  .quick-action:hover :global(svg),
  .quick-action:focus-visible :global(svg) {
    opacity: 1;
  }

  .message-content :global(a.citation) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    margin-inline: 2px;
    padding-inline: 3px;
    border-radius: var(--radius-xs);
    background: var(--color-hover-bg);
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
    font-weight: var(--weight-medium);
    font-variant-numeric: tabular-nums;
    line-height: 1;
    vertical-align: 0.15em;
    white-space: nowrap;
    text-decoration: none;
  }
  /* Same badge the composer shows while typing (docs: `.composer-backdrop
     :global(.mention-highlight)`), so a reference looks the same before and
     after sending - minus the negative-margin trick, which only exists
     there to keep the invisible textarea's line-wrapping unaffected. */
  .message-content :global(a.mention-highlight) {
    padding: 1px 3px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--color-accent) 16%, transparent);
    color: var(--color-accent);
    text-decoration: none;
  }
  .message-content :global(a.citation:hover),
  .message-content :global(a.answer-source:hover) {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }
  .message-content :global(a.mention-highlight:hover) {
    background: color-mix(in srgb, var(--color-accent) 28%, transparent);
  }
  .message-content :global(a.citation:focus-visible),
  .message-content :global(a.answer-source:focus-visible),
  .message-content :global(a.mention-highlight:focus-visible) {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  .message-content :global(.answer-sources) {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-4);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }
  .message-content :global(.sources-label) {
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }
  .message-content :global(a.answer-source) {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
    padding: var(--space-1);
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    text-decoration: none;
  }
  .message-content :global(.source-number) {
    flex-shrink: 0;
    min-width: 16px;
    text-align: center;
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }
  .message-content :global(.source-title) {
    display: -webkit-box;
    overflow: hidden;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .confirm-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    min-height: var(--control-size);
    padding: var(--space-2);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.4;
  }
  .confirm-note {
    min-width: 0;
  }
  .confirm-actions {
    display: flex;
    flex-shrink: 0;
    gap: var(--space-1);
  }
  .provider-switch-confirm {
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
  }

  .composer-anchor {
    position: relative;
  }
</style>
