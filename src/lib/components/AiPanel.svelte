<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import {
    cancelAiMessage,
    deleteAiSession,
    listAiSessions,
    loadAiTranscript,
    startAiMessage,
  } from '$lib/api';
  import Icon from '$lib/icons/Icon.svelte';
  import { formatRelative } from '$lib/format';
  import { t } from '$lib/i18n/index.svelte';
  import { renderMarkdown } from '$lib/markdown';
  import type { AiMessage, AiProvider, AiSession, AiStreamEvent, Item } from '$lib/types';

  interface Props {
    item: Item | null;
    /** The post's feed language tag (e.g. "ko"), if the feed declares one. */
    language?: string | null;
    onCollapse: () => void;
    pendingQuote?: string | null;
    onConsumedPendingQuote?: () => void;
  }

  let {
    item,
    language = null,
    onCollapse,
    pendingQuote = null,
    onConsumedPendingQuote,
  }: Props = $props();

  const PROVIDER_KEY = 'aequo:aiProvider';
  const PROVIDER_NAMES: Record<AiProvider, string> = { codex: 'Codex', claude: 'Claude Code' };

  function readStoredProvider(): AiProvider {
    try {
      return localStorage.getItem(PROVIDER_KEY) === 'claude' ? 'claude' : 'codex';
    } catch {
      return 'codex';
    }
  }

  /** The in-flight answer for the open post (one at a time per panel). */
  interface Run {
    requestId: string | null;
    postId: number;
    prompt: string;
    text: string;
    status: string | null;
  }

  let provider = $state<AiProvider>(readStoredProvider());
  let sessions = $state<AiSession[]>([]);
  let messages = $state<AiMessage[]>([]);
  let selectedSessionId = $state<number | null>(null);
  let draft = $state('');
  let quote = $state<string | null>(null);
  let run = $state<Run | null>(null);
  let loadingMessages = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let transcriptNote = $state<string | null>(null);
  let lastPrompt = $state('');
  let copiedMessageId = $state<number | null>(null);
  let historyOpen = $state(false);
  let confirmingRemoveId = $state<number | null>(null);
  // Set after switching provider mid-conversation: lets the user jump back.
  let previousConversation = $state<{ provider: AiProvider; sessionId: number } | null>(null);
  let messageList = $state<HTMLDivElement | undefined>(undefined);
  let composerEl = $state<HTMLTextAreaElement | undefined>(undefined);
  let contextGeneration = 0;
  let loadGeneration = 0;

  // Streaming text is collected outside reactive state and flushed once per
  // animation frame, so a fast stream re-renders the Markdown at most ~60x/s.
  let streamBuffer = '';
  let flushScheduled = false;
  // Events can arrive before start_ai_message has returned the request id.
  let earlyEvents: AiStreamEvent[] = [];
  let stickToBottom = true;

  const sending = $derived(run !== null && run.postId === item?.id);
  const isEmptyChat = $derived(
    !selectedSessionId && messages.length === 0 && !sending && !loadingMessages,
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

  // A different post starts from an empty chat. A run still streaming for
  // the previous post keeps going in the backend and lands in that post's
  // history; this panel just stops showing it.
  $effect(() => {
    const postId = item?.id;
    const generation = ++contextGeneration;
    loadGeneration += 1;
    run = null;
    selectedSessionId = null;
    messages = [];
    sessions = [];
    error = null;
    notice = null;
    transcriptNote = null;
    historyOpen = false;
    previousConversation = null;
    if (postId === undefined) return;
    loadAllSessions(postId)
      .then((result) => {
        if (generation === contextGeneration) sessions = result;
      })
      .catch((reason) => {
        if (generation === contextGeneration) error = String(reason);
      });
  });

  // "Ask AI about selection" (docs/READER_ANNOTATIONS_PLAN.md §4.3) always
  // starts a new conversation with the selection as a quote chip.
  $effect(() => {
    if (!pendingQuote) return;
    const selected = pendingQuote;
    startNewChat();
    quote = selected;
    queueMicrotask(() => composerEl?.focus());
    onConsumedPendingQuote?.();
  });

  onMount(() => {
    composerEl?.focus();
    const unlisten = listen<AiStreamEvent>('ai-stream', (event) =>
      handleStreamEvent(event.payload),
    );
    return () => {
      unlisten.then((stop) => stop());
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
    flushScheduled = false;
    if (run) run.text = streamBuffer;
    scrollToEnd();
  }

  function handleStreamEvent(event: AiStreamEvent) {
    if (!run) return;
    if (run.requestId === null) {
      earlyEvents.push(event);
      return;
    }
    if (event.requestId !== run.requestId) return;
    switch (event.kind) {
      case 'delta':
        streamBuffer += event.text;
        if (!flushScheduled) {
          flushScheduled = true;
          requestAnimationFrame(flushStream);
        }
        break;
      case 'replace':
        streamBuffer = event.text;
        flushStream();
        break;
      case 'status':
        run.status = event.text;
        break;
      default:
        finishRun(event);
    }
  }

  async function finishRun(event: AiStreamEvent) {
    const finished = run;
    if (!finished) return;
    const finalText = event.kind === 'done' ? event.text || streamBuffer : streamBuffer;
    run = null;
    streamBuffer = '';
    const postId = finished.postId;
    const generation = contextGeneration;
    selectedSessionId = event.sessionId;

    const localTurn: AiMessage[] = [
      localMessage('user', finished.prompt, event.sessionId),
      ...(finalText ? [localMessage('assistant', finalText, event.sessionId)] : []),
    ];

    if (event.kind === 'error') {
      error = event.text;
      messages = [...messages, localTurn[0]];
    } else if (event.kind === 'cancelled') {
      // The CLI transcript won't have a stopped answer; keep what streamed.
      notice = t('ai.stopped');
      messages = [...messages, ...localTurn];
    } else {
      try {
        const transcript = await loadAiTranscript(event.sessionId);
        if (generation !== contextGeneration) return;
        transcriptNote = transcript.note;
        messages =
          transcript.messages.length > 0 ? transcript.messages : [...messages, ...localTurn];
      } catch {
        if (generation === contextGeneration) messages = [...messages, ...localTurn];
      }
    }
    scrollToEnd(true);
    try {
      const updated = await loadAllSessions(postId);
      if (generation === contextGeneration) sessions = updated;
    } catch {
      // History refresh is best-effort; the conversation itself is shown.
    }
  }

  function localMessage(role: 'user' | 'assistant', content: string, sessionId: number): AiMessage {
    return {
      id: Date.now() + (role === 'user' ? 0 : 1),
      sessionId,
      role,
      content,
      createdAt: Math.floor(Date.now() / 1000),
    };
  }

  function startNewChat() {
    selectedSessionId = null;
    messages = [];
    error = null;
    notice = null;
    lastPrompt = '';
    transcriptNote = null;
    historyOpen = false;
  }

  function selectProvider(next: AiProvider) {
    if (provider === next || sending) return;
    // A conversation belongs to one CLI, so switching starts a new one; the
    // old one stays in history and one click brings it back (§3.9).
    if (selectedSessionId !== null) {
      previousConversation = { provider, sessionId: selectedSessionId };
    }
    provider = next;
    try {
      localStorage.setItem(PROVIDER_KEY, next);
    } catch {
      // The choice still applies for this session.
    }
    startNewChat();
  }

  async function openSession(session: AiSession) {
    if (sending) return;
    const generation = ++loadGeneration;
    selectedSessionId = session.id;
    provider = session.provider;
    previousConversation = null;
    historyOpen = false;
    loadingMessages = true;
    messages = [];
    transcriptNote = null;
    error = null;
    notice = null;
    try {
      const result = await loadAiTranscript(session.id);
      if (generation === loadGeneration) {
        messages = result.messages;
        transcriptNote = result.note;
        scrollToEnd(true);
      }
    } catch (reason) {
      if (generation === loadGeneration) error = String(reason);
    } finally {
      if (generation === loadGeneration) loadingMessages = false;
    }
  }

  function goBackToPrevious() {
    const target = sessions.find((session) => session.id === previousConversation?.sessionId);
    if (target) openSession(target);
  }

  async function removeSession(session: AiSession) {
    if (sending) return;
    try {
      await deleteAiSession(session.id);
      if (selectedSessionId === session.id) startNewChat();
      confirmingRemoveId = null;
      if (item) sessions = await loadAllSessions(item.id);
    } catch (reason) {
      error = String(reason);
    }
  }

  // The answer's language follows the article (§3.15): the feed's declared
  // language when known, otherwise whatever the article is written in.
  function languageDirective(): string {
    return language
      ? `Respond in the language with BCP 47 tag "${language}".`
      : 'Respond in the same language the article is written in.';
  }

  function postContext(): string {
    if (!item) return '';
    return [item.title ? `Title: ${item.title}` : '', item.link ? `Post URL: ${item.link}` : '']
      .filter(Boolean)
      .join('\n');
  }

  function askChip(question: string) {
    submitMessage(`${question} ${languageDirective()}\n\n${postContext()}`);
  }

  async function submitMessage(text = draft.trim()) {
    if (!item || sending) return;
    const quoted = quote;
    if (!text && !quoted) return;
    const prompt = [quoted ? quoted.replace(/^/gm, '> ') : '', text].filter(Boolean).join('\n\n');
    const postId = item.id;
    const generation = contextGeneration;

    run = { requestId: null, postId, prompt, text: '', status: null };
    streamBuffer = '';
    earlyEvents = [];
    draft = '';
    quote = null;
    error = null;
    notice = null;
    lastPrompt = prompt;
    previousConversation = null;
    stickToBottom = true;
    scrollToEnd(true);
    if (composerEl) composerEl.style.height = '';

    try {
      const started = await startAiMessage(postId, provider, selectedSessionId, prompt);
      if (generation !== contextGeneration || !run || run.postId !== postId) return;
      run.requestId = started.requestId;
      selectedSessionId = started.sessionId;
      const queued = earlyEvents;
      earlyEvents = [];
      for (const event of queued) handleStreamEvent(event);
    } catch (reason) {
      if (generation !== contextGeneration) return;
      run = null;
      error = String(reason);
    }
  }

  function stopRun() {
    if (run?.requestId) cancelAiMessage(run.requestId);
  }

  function resizeComposer(event: Event) {
    const textarea = event.currentTarget as HTMLTextAreaElement;
    textarea.style.height = 'auto';
    textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`;
  }

  async function copyMessage(message: AiMessage) {
    try {
      await navigator.clipboard.writeText(message.content);
      copiedMessageId = message.id;
      window.setTimeout(() => {
        if (copiedMessageId === message.id) copiedMessageId = null;
      }, 1400);
    } catch {
      error = t('ai.copyFailed');
    }
  }

  function clickOutside(node: HTMLElement, onOutside: () => void) {
    function handle(event: MouseEvent) {
      if (!node.contains(event.target as Node)) onOutside();
    }
    document.addEventListener('mousedown', handle, true);
    return {
      destroy() {
        document.removeEventListener('mousedown', handle, true);
      },
    };
  }
</script>

<aside class="ai-panel" aria-label={t('ai.panel')}>
  <header class="panel-header">
    <select
      class="provider"
      aria-label={t('ai.provider')}
      value={provider}
      disabled={sending}
      onchange={(event) => selectProvider(event.currentTarget.value as AiProvider)}
    >
      <option value="codex">{PROVIDER_NAMES.codex}</option>
      <option value="claude">{PROVIDER_NAMES.claude}</option>
    </select>
    <div class="header-actions">
      <div class="history-anchor">
        <button
          class="icon-button"
          type="button"
          aria-label={t('ai.history')}
          title={t('ai.history')}
          aria-haspopup="dialog"
          aria-expanded={historyOpen}
          disabled={!item}
          onclick={() => (historyOpen = !historyOpen)}><Icon name="clock" size={15} /></button
        >
        {#if historyOpen}
          <div
            class="history-popover"
            role="dialog"
            aria-label={t('ai.history')}
            use:clickOutside={() => (historyOpen = false)}
          >
            {#if sessions.length === 0}
              <p class="history-empty">{t('ai.historyEmpty')}</p>
            {:else}
              <ul>
                {#each sessions as session (session.id)}
                  <li class:current={selectedSessionId === session.id}>
                    {#if confirmingRemoveId === session.id}
                      <div class="confirm-row">
                        <span>{t('ai.historyNote')}</span>
                        <div class="confirm-actions">
                          <button type="button" onclick={() => (confirmingRemoveId = null)}
                            >{t('common.cancel')}</button
                          >
                          <button
                            type="button"
                            class="confirm-remove"
                            onclick={() => removeSession(session)}>{t('ai.remove')}</button
                          >
                        </div>
                      </div>
                    {:else}
                      <button
                        class="history-open"
                        type="button"
                        disabled={sending}
                        onclick={() => openSession(session)}
                      >
                        <span class="history-title">{session.title}</span>
                        <span class="history-meta"
                          >{PROVIDER_NAMES[session.provider]} · {formatRelative(
                            session.updatedAt,
                          )}</span
                        >
                      </button>
                      <button
                        class="icon-button small"
                        type="button"
                        aria-label={t('ai.removeConversation')}
                        title={t('ai.removeConversation')}
                        disabled={sending}
                        onclick={() => (confirmingRemoveId = session.id)}
                        ><Icon name="close" size={12} /></button
                      >
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {/if}
      </div>
      <button
        class="icon-button"
        type="button"
        aria-label={t('ai.newChat')}
        title={t('ai.newChat')}
        disabled={!item || sending || isEmptyChat}
        onclick={startNewChat}><Icon name="edit" size={15} /></button
      >
      <button
        class="icon-button"
        type="button"
        aria-label={t('ai.close')}
        title={t('ai.close')}
        onclick={onCollapse}><Icon name="close" size={15} /></button
      >
    </div>
  </header>

  <div class="chat-messages" bind:this={messageList} onscroll={handleScroll}>
    {#if previousConversation}
      <p class="switch-note">
        {t('ai.switchedProvider', { provider: PROVIDER_NAMES[previousConversation.provider] })}
        · <button type="button" onclick={goBackToPrevious}>{t('ai.backToConversation')}</button>
      </p>
    {/if}

    {#if isEmptyChat}
      <div class="chat-empty">
        <div class="chips">
          <button
            type="button"
            disabled={!item?.link}
            title={item?.link ? undefined : t('ai.chip.needsLink')}
            onclick={() => askChip('Summarize this post: the main idea and the key points.')}
            >{t('ai.chip.summarize')}</button
          >
          <button type="button" onclick={() => askChip('What are the key takeaways?')}
            >{t('ai.chip.takeaways')}</button
          >
          <button type="button" onclick={() => askChip('Explain this post in simple terms.')}
            >{t('ai.chip.explain')}</button
          >
        </div>
        {#if sessions.length > 0}
          <div class="previous">
            <p class="previous-label">{t('ai.previous')}</p>
            {#each sessions.slice(0, 3) as session (session.id)}
              <button type="button" class="previous-item" onclick={() => openSession(session)}>
                <span class="history-title">{session.title}</span>
                <span class="history-meta"
                  >{PROVIDER_NAMES[session.provider]} · {formatRelative(session.updatedAt)}</span
                >
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else if loadingMessages}
      <p class="subtle">{t('ai.loading')}</p>
    {:else if selectedSessionId && messages.length === 0 && !sending}
      <p class="subtle" role="status">{transcriptNote ?? t('ai.transcriptMissing')}</p>
    {/if}

    {#each messages as message (message.id)}
      {#if message.role === 'user'}
        <div class="message user">
          <span class="visually-hidden">{t('ai.you')}</span>
          <!-- eslint-disable-next-line svelte/no-at-html-tags -->
          <div class="message-content">{@html renderMarkdown(message.content)}</div>
        </div>
      {:else}
        <div class="message assistant">
          <span class="visually-hidden">{PROVIDER_NAMES[provider]}</span>
          <!-- eslint-disable-next-line svelte/no-at-html-tags -->
          <div class="message-content">{@html renderMarkdown(message.content)}</div>
          <button
            class="icon-button small copy"
            type="button"
            aria-label={copiedMessageId === message.id ? t('ai.copied') : t('ai.copy')}
            title={copiedMessageId === message.id ? t('ai.copied') : t('ai.copy')}
            onclick={() => copyMessage(message)}
            ><Icon name={copiedMessageId === message.id ? 'check' : 'copy'} size={13} /></button
          >
        </div>
      {/if}
    {/each}

    {#if run && sending}
      <div class="message user">
        <!-- eslint-disable-next-line svelte/no-at-html-tags -->
        <div class="message-content">{@html renderMarkdown(run.prompt)}</div>
      </div>
      <div class="message assistant" aria-live="polite" aria-busy="true">
        {#if run.text}
          <!-- eslint-disable-next-line svelte/no-at-html-tags -->
          <div class="message-content">{@html renderMarkdown(run.text)}</div>
        {:else}
          <div class="thinking">
            <span></span><span></span><span></span>
            {t(
              run.status === 'thinking'
                ? 'ai.status.thinking'
                : run.status === 'tool'
                  ? 'ai.status.tool'
                  : 'ai.status.working',
            )}
          </div>
        {/if}
      </div>
    {/if}

    {#if transcriptNote && selectedSessionId && messages.length > 0}
      <p class="subtle" role="status">{transcriptNote}</p>
    {/if}
    {#if notice}
      <p class="subtle" role="status">{notice}</p>
    {/if}
  </div>

  {#if error}
    <div class="error-card" role="alert">
      <span class="error-mark"><Icon name="error" size={12} /></span>
      <div class="error-copy">
        <strong>{t('ai.errorTitle')}</strong>
        <p>{error}</p>
      </div>
      {#if lastPrompt && item && !sending}<button
          type="button"
          onclick={() => submitMessage(lastPrompt)}>{t('ai.retry')}</button
        >{/if}
      <button
        class="dismiss-error"
        type="button"
        aria-label={t('ai.dismissError')}
        onclick={() => (error = null)}><Icon name="close" size={13} /></button
      >
    </div>
  {/if}

  <form
    class="composer"
    onsubmit={(event) => {
      event.preventDefault();
      submitMessage();
    }}
  >
    {#if quote}
      <div class="quote-chip">
        <p>{quote}</p>
        <button
          class="icon-button small"
          type="button"
          aria-label={t('ai.removeQuote')}
          title={t('ai.removeQuote')}
          onclick={() => (quote = null)}><Icon name="close" size={12} /></button
        >
      </div>
    {/if}
    <div class="composer-row">
      <textarea
        bind:value={draft}
        bind:this={composerEl}
        rows="1"
        aria-label={t('ai.composer')}
        placeholder={t('ai.placeholder')}
        disabled={!item}
        oninput={resizeComposer}
        onkeydown={(event) => {
          // Enter sends, Shift+Enter breaks the line; never while an IME
          // (e.g. Korean) is still composing a character.
          if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
            event.preventDefault();
            submitMessage();
          }
        }}></textarea>
      {#if sending}
        <button
          class="send stop"
          type="button"
          aria-label={t('ai.stop')}
          title={t('ai.stop')}
          disabled={!run?.requestId}
          onclick={stopRun}><Icon name="stop" size={14} /></button
        >
      {:else}
        <button
          class="send"
          type="submit"
          aria-label={t('ai.send')}
          title={t('ai.send')}
          disabled={!item || (!draft.trim() && !quote)}><Icon name="send" size={14} /></button
        >
      {/if}
    </div>
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
    display: flex;
    min-height: 48px;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  .provider {
    min-width: 0;
    padding: 4px 6px;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
    cursor: pointer;
  }

  .provider:hover:not(:disabled) {
    background: var(--color-hover-bg);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .icon-button {
    display: grid;
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
  }

  .icon-button.small {
    width: 24px;
    height: 24px;
  }

  .icon-button:hover:not(:disabled) {
    background: var(--color-hover-bg);
    color: var(--color-text);
  }

  .icon-button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .history-anchor {
    position: relative;
  }

  .history-popover {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 30;
    width: 300px;
    max-height: 360px;
    overflow-y: auto;
    padding: var(--space-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    box-shadow: var(--shadow-popover);
  }

  .history-popover ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .history-popover li {
    display: flex;
    align-items: center;
    gap: 2px;
    border-radius: var(--radius-xs);
  }

  .history-popover li.current {
    background: var(--color-selection-bg);
  }

  .history-popover .confirm-row {
    flex: 1;
    margin: var(--space-1);
  }

  .history-open,
  .previous-item {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    padding: 6px var(--space-2);
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .history-open:hover:not(:disabled),
  .previous-item:hover {
    background: var(--color-hover-bg);
  }

  .history-title {
    overflow: hidden;
    font-size: var(--text-body);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .history-meta {
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }

  .history-empty {
    margin: var(--space-3);
    color: var(--color-text-secondary);
    font-size: var(--text-small);
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

  .switch-note button {
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
    gap: var(--space-5);
    margin-top: auto;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .chips button {
    padding: 6px var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-pill);
    background: var(--color-bg);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  .chips button:hover:not(:disabled) {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .chips button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .previous {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .previous-label {
    margin: 0 var(--space-2) var(--space-1);
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }

  .subtle {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.5;
  }

  .message {
    position: relative;
    min-width: 0;
  }

  .message.user {
    align-self: flex-end;
    max-width: 85%;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-lg);
    background: var(--color-hover-bg);
  }

  .message.assistant {
    padding-right: var(--space-5);
  }

  .message .copy {
    position: absolute;
    top: -4px;
    right: 0;
    opacity: 0;
  }

  .message.assistant:hover .copy,
  .message .copy:focus-visible {
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
  .message-content :global(pre) {
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
    margin: 2px 0;
  }

  .message-content :global(code) {
    padding: 1px 4px;
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

  .message-content :global(a) {
    color: var(--color-accent);
  }

  .thinking {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
  }

  .thinking span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    animation: pulse 1.2s infinite ease-in-out;
  }

  .thinking span:nth-child(2) {
    animation-delay: 0.15s;
  }

  .thinking span:nth-child(3) {
    margin-right: var(--space-1);
    animation-delay: 0.3s;
  }

  @keyframes pulse {
    0%,
    80%,
    100% {
      opacity: 0.25;
    }
    40% {
      opacity: 1;
    }
  }

  .error-card {
    margin: 0 var(--space-3) var(--space-2);
  }

  .composer {
    padding: var(--space-2) var(--space-3) var(--space-3);
  }

  .quote-chip {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
    padding: var(--space-1) var(--space-1) var(--space-1) var(--space-2);
    border-left: 3px solid var(--color-accent);
    border-radius: var(--radius-xs);
    background: var(--color-bg-secondary);
  }

  .quote-chip p {
    display: -webkit-box;
    min-width: 0;
    flex: 1;
    margin: 2px 0;
    overflow: hidden;
    color: var(--color-text-secondary);
    font-size: var(--text-small);
    line-height: 1.45;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }

  .composer-row {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-1) var(--space-1) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
  }

  .composer-row:focus-within {
    border-color: var(--color-accent);
  }

  textarea {
    min-width: 0;
    flex: 1;
    max-height: 180px;
    padding: 6px 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
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
    width: 30px;
    height: 30px;
    flex-shrink: 0;
    place-items: center;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
    cursor: pointer;
  }

  .send:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .send.stop {
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
</style>
