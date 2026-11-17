<script lang="ts">
  import { deleteAiSession, listAiSessions, loadAiTranscript, sendAiMessage } from '$lib/api';
  import Icon from '$lib/icons/Icon.svelte';
  import type { AiMessage, AiProvider, AiSession, Item } from '$lib/types';

  interface Props {
    item: Item | null;
    onCollapse: () => void;
    pendingQuote?: string | null;
    onConsumedPendingQuote?: () => void;
  }

  let { item, onCollapse, pendingQuote = null, onConsumedPendingQuote }: Props = $props();

  let provider = $state<AiProvider>('codex');
  let activeTab = $state<'chat' | 'history'>('chat');
  let sessions = $state<AiSession[]>([]);
  let messages = $state<AiMessage[]>([]);
  let selectedSessionId = $state<number | null>(null);
  let draft = $state('');
  let sending = $state(false);
  let loadingSessions = $state(false);
  let loadingMessages = $state(false);
  let error = $state<string | null>(null);
  let transcriptNote = $state<string | null>(null);
  let lastPrompt = $state('');
  let pendingPrompt = $state('');
  let copiedMessageId = $state<number | null>(null);
  let confirmingRemoveId = $state<number | null>(null);
  let messageList = $state<HTMLDivElement | undefined>(undefined);
  let composerEl = $state<HTMLTextAreaElement | undefined>(undefined);
  let loadGeneration = 0;
  let contextGeneration = 0;

  async function loadAllSessions(postId: number): Promise<AiSession[]> {
    const [codexSessions, claudeSessions] = await Promise.all([
      listAiSessions(postId, 'codex'),
      listAiSessions(postId, 'claude'),
    ]);
    return [...codexSessions, ...claudeSessions].sort(
      (a, b) => b.updatedAt - a.updatedAt || b.id - a.id,
    );
  }

  $effect(() => {
    const postId = item?.id;
    const generation = ++contextGeneration;
    let cancelled = false;
    loadGeneration += 1;
    sending = false;
    pendingPrompt = '';
    selectedSessionId = null;
    messages = [];
    sessions = [];
    error = null;
    transcriptNote = null;

    if (postId === undefined) return;
    loadingSessions = true;
    loadAllSessions(postId)
      .then((result) => {
        if (!cancelled && generation === contextGeneration) sessions = result;
      })
      .catch((reason) => {
        if (!cancelled && generation === contextGeneration) error = String(reason);
      })
      .finally(() => {
        if (!cancelled && generation === contextGeneration) loadingSessions = false;
      });

    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (messages.length >= 0 && messageList) {
      queueMicrotask(() => {
        if (messageList) messageList.scrollTop = messageList.scrollHeight;
      });
    }
  });

  // "Ask AI about selection" (docs/READER_ANNOTATIONS_PLAN.md §4.3): a quote
  // selected in the reader always starts a brand-new session here, never
  // appends to whatever session happens to be open. `pendingQuote` is a
  // one-shot value - consumed exactly once via onConsumedPendingQuote so
  // reopening/toggling the panel afterward doesn't re-trigger the prefill.
  $effect(() => {
    if (!pendingQuote) return;
    const quote = pendingQuote;
    startNewChat();
    draft = `> ${quote}\n\n`;
    queueMicrotask(() => composerEl?.focus());
    onConsumedPendingQuote?.();
  });

  function selectProvider(next: AiProvider) {
    if (provider === next) return;
    provider = next;
    startNewChat();
  }

  function startNewChat() {
    selectedSessionId = null;
    messages = [];
    error = null;
    lastPrompt = '';
    transcriptNote = null;
    activeTab = 'chat';
  }

  async function openSession(session: AiSession) {
    if (sending) return;
    const generation = ++loadGeneration;
    selectedSessionId = session.id;
    provider = session.provider;
    activeTab = 'chat';
    loadingMessages = true;
    messages = [];
    transcriptNote = null;
    error = null;
    try {
      const result = await loadAiTranscript(session.id);
      if (generation === loadGeneration) {
        messages = result.messages;
        transcriptNote = result.note;
      }
    } catch (reason) {
      if (generation === loadGeneration) error = String(reason);
    } finally {
      if (generation === loadGeneration) loadingMessages = false;
    }
  }

  async function reloadSessions() {
    if (!item) return;
    sessions = await loadAllSessions(item.id);
  }

  function requestRemoveSession(session: AiSession) {
    if (sending) return;
    confirmingRemoveId = session.id;
  }

  function cancelRemoveSession() {
    confirmingRemoveId = null;
  }

  async function removeSession(session: AiSession) {
    if (sending) return;
    try {
      await deleteAiSession(session.id);
      if (selectedSessionId === session.id) startNewChat();
      confirmingRemoveId = null;
      await reloadSessions();
    } catch (reason) {
      error = String(reason);
    }
  }

  async function submitMessage(prompt = draft.trim()) {
    if (!item || !prompt || sending) return;
    const postId = item.id;
    const currentProvider = provider;
    const currentSessionId = selectedSessionId;
    const generation = contextGeneration;
    sending = true;
    error = null;
    draft = '';
    lastPrompt = prompt;
    pendingPrompt = prompt;

    try {
      const result = await sendAiMessage(postId, currentProvider, currentSessionId, prompt);
      if (generation !== contextGeneration) return;
      selectedSessionId = result.sessionId;
      const transcript = await loadAiTranscript(result.sessionId);
      if (generation !== contextGeneration) return;
      transcriptNote = transcript.note;
      if (transcript.messages.length > 0) {
        messages = transcript.messages;
      } else {
        const userMessage: AiMessage = {
          id: Date.now(),
          sessionId: result.sessionId,
          role: 'user',
          content: prompt,
          createdAt: Math.floor(Date.now() / 1000),
        };
        messages = [...messages, userMessage, result.assistantMessage];
      }
      sessions = await loadAllSessions(postId);
    } catch (reason) {
      if (generation !== contextGeneration) return;
      error = String(reason);
      try {
        const updatedSessions = await loadAllSessions(postId);
        if (generation !== contextGeneration) return;
        sessions = updatedSessions;
        const providerSessions = updatedSessions.filter(
          (session) => session.provider === currentProvider,
        );
        if (currentSessionId === null && providerSessions.length > 0) {
          const newest = providerSessions[0];
          selectedSessionId = newest.id;
          const transcript = await loadAiTranscript(newest.id);
          messages = transcript.messages;
          transcriptNote = transcript.note;
        } else if (currentSessionId !== null) {
          const transcript = await loadAiTranscript(currentSessionId);
          messages = transcript.messages;
          transcriptNote = transcript.note;
        }
      } catch {
        // Keep the command error visible when history refresh also fails.
      }
    } finally {
      if (generation === contextGeneration) {
        sending = false;
        pendingPrompt = '';
      }
    }
  }

  function summarizePost() {
    if (!item?.link) return;
    const title = item.title?.trim();
    const prompt = `Summarize it. Give me the main idea and key points.\n\n${title ? `Title: ${title}\n` : ''}Post URL: ${item.link}`;
    submitMessage(prompt);
  }

  function askAboutPost(question: string) {
    if (!item) return;
    const context = [
      item.title ? `Title: ${item.title}` : '',
      item.link ? `Post URL: ${item.link}` : '',
    ]
      .filter(Boolean)
      .join('\n');
    submitMessage(context ? `${question}\n\n${context}` : question);
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
      error = 'Could not copy this response. Please select and copy the text instead.';
    }
  }

  function formatTime(timestamp: number): string {
    return new Date(timestamp * 1000).toLocaleString(undefined, {
      dateStyle: 'short',
      timeStyle: 'short',
    });
  }

  function escapeHtml(value: string): string {
    return value.replace(/[&<>"']/g, (char) => {
      const entities: Record<string, string> = {
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;',
      };
      return entities[char];
    });
  }

  function inlineMarkdown(value: string): string {
    let safe = escapeHtml(value);
    safe = safe.replace(/`([^`]+)`/g, '<code>$1</code>');
    safe = safe.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    safe = safe.replace(/__([^_]+)__/g, '<strong>$1</strong>');
    safe = safe.replace(/\*([^*]+)\*/g, '<em>$1</em>');
    safe = safe.replace(/_([^_]+)_/g, '<em>$1</em>');
    safe = safe.replace(
      /(^|\s)(https?:\/\/[^\s<]+)/g,
      '$1<a href="$2" target="_blank" rel="noopener noreferrer">$2</a>',
    );
    safe = safe.replace(
      /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
      (_match, label: string, url: string) =>
        `<a href="${url}" target="_blank" rel="noopener noreferrer">${label}</a>`,
    );
    return safe;
  }

  function renderMarkdown(markdown: string): string {
    const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
    const blocks: string[] = [];
    let paragraph: string[] = [];
    let list: string[] = [];
    let listKind: 'ul' | 'ol' | null = null;
    let code: string[] = [];
    let inCode = false;

    const flushParagraph = () => {
      if (paragraph.length) blocks.push(`<p>${paragraph.map(inlineMarkdown).join('<br>')}</p>`);
      paragraph = [];
    };
    const flushList = () => {
      if (listKind && list.length)
        blocks.push(
          `<${listKind}>${list.map((item) => `<li>${inlineMarkdown(item)}</li>`).join('')}</${listKind}>`,
        );
      list = [];
      listKind = null;
    };

    for (const line of lines) {
      if (/^\s*```/.test(line)) {
        flushParagraph();
        flushList();
        if (inCode) {
          blocks.push(`<pre><code>${escapeHtml(code.join('\n'))}</code></pre>`);
          code = [];
          inCode = false;
        } else {
          inCode = true;
        }
        continue;
      }
      if (inCode) {
        code.push(line);
        continue;
      }
      if (!line.trim()) {
        flushParagraph();
        flushList();
        continue;
      }

      const heading = line.match(/^\s{0,3}(#{1,4})\s+(.+)$/);
      if (heading) {
        flushParagraph();
        flushList();
        const level = heading[1].length;
        blocks.push(`<h${level}>${inlineMarkdown(heading[2])}</h${level}>`);
        continue;
      }

      const bullet = line.match(/^\s*[-*+]\s+(.+)$/);
      const numbered = line.match(/^\s*\d+[.)]\s+(.+)$/);
      const item = bullet?.[1] ?? numbered?.[1];
      const kind = bullet ? 'ul' : numbered ? 'ol' : null;
      if (item && kind) {
        flushParagraph();
        if (listKind && listKind !== kind) flushList();
        listKind = kind;
        list.push(item);
        continue;
      }

      flushList();
      paragraph.push(line);
    }

    if (inCode) blocks.push(`<pre><code>${escapeHtml(code.join('\n'))}</code></pre>`);
    flushParagraph();
    flushList();
    return blocks.join('');
  }
</script>

<aside class="ai-panel" aria-label="AI assistant">
  <header class="panel-header">
    <div class="panel-title">
      <span class="spark"><Icon name="ask-ai" size={14} /></span>
      <h2>AI assistant</h2>
      <span class="local-status"><i></i> Local</span>
      <button
        class="collapse-panel"
        type="button"
        aria-label="Collapse AI panel"
        title="Collapse AI panel"
        onclick={onCollapse}><Icon name="chevron-right" size={14} /></button
      >
    </div>
    <div class="tabs" role="tablist" aria-label="AI panel view">
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === 'chat'}
        class:active={activeTab === 'chat'}
        onclick={() => (activeTab = 'chat')}>Chat</button
      >
      <button
        type="button"
        role="tab"
        aria-selected={activeTab === 'history'}
        class:active={activeTab === 'history'}
        disabled={!item}
        onclick={() => (activeTab = 'history')}>History</button
      >
    </div>
  </header>

  {#if activeTab === 'chat'}
    <div class="provider-row">
      <span class="provider-label">ASSISTANT</span>
    </div>
    <div class="provider-picker" aria-label="AI provider">
      <button
        type="button"
        class:active={provider === 'codex'}
        aria-pressed={provider === 'codex'}
        disabled={sending}
        onclick={() => selectProvider('codex')}>Codex</button
      >
      <button
        type="button"
        class:active={provider === 'claude'}
        aria-pressed={provider === 'claude'}
        disabled={sending}
        onclick={() => selectProvider('claude')}>Claude</button
      >
      <button class="new-chat" type="button" disabled={!item || sending} onclick={startNewChat}>
        New chat
      </button>
    </div>
  {/if}

  {#if activeTab === 'chat'}
    {#if item}
      <div class="post-context" title={item.title ?? ''}>
        <div class="context-copy">
          <span class="context-label">DISCUSSING</span>
          <span class="context-title">{item.title ?? '(untitled)'}</span>
        </div>
        {#if item.link}
          <!-- eslint-disable svelte/no-navigation-without-resolve -->
          <a
            class="context-link"
            href={item.link}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Open original post"><Icon name="external-link" size={13} /></a
          >
          <!-- eslint-enable svelte/no-navigation-without-resolve -->
        {/if}
        <span class="conversation-state">{selectedSessionId ? 'Continuing' : 'New chat'}</span>
      </div>

      <div class="chat-messages" bind:this={messageList}>
        {#if !selectedSessionId && messages.length === 0}
          <div class="chat-empty">
            <div class="empty-icon"><Icon name="ask-ai" size={22} /></div>
            <h3>What would you like to know?</h3>
            <p>Ask a question about this post or choose a quick start.</p>
            <button
              class="summarize"
              type="button"
              disabled={!item.link || sending}
              onclick={summarizePost}
            >
              <Icon name="ask-ai" size={13} />
              Summarize
            </button>
            {#if !item.link}<p class="hint">This post has no original URL.</p>{/if}
            <div class="suggestions" aria-label="Suggested questions">
              <button
                type="button"
                disabled={sending}
                onclick={() => askAboutPost('What are the key takeaways?')}>Key takeaways</button
              >
              <button
                type="button"
                disabled={sending}
                onclick={() => askAboutPost('Explain this in simple terms.')}>Explain simply</button
              >
              <button
                type="button"
                disabled={sending}
                onclick={() => askAboutPost('What questions should I ask about this?')}
                >Explore further</button
              >
            </div>
          </div>
        {:else if loadingMessages}
          <p class="subtle">Loading conversation…</p>
        {:else if selectedSessionId && messages.length === 0}
          <p class="transcript-empty" role="status">
            {transcriptNote ??
              'No prior messages are available in the local CLI transcript. You can still continue this session.'}
          </p>
        {/if}

        {#if sending && pendingPrompt}
          <div class="message user pending-user">
            <span class="message-role">You</span>
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            <div class="message-content">{@html renderMarkdown(pendingPrompt)}</div>
          </div>
        {/if}
        {#each messages as message (message.id)}
          <div class="message" class:user={message.role === 'user'}>
            <span class="message-role"
              >{message.role === 'user' ? 'You' : provider === 'codex' ? 'Codex' : 'Claude'}</span
            >
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            <div class="message-content">{@html renderMarkdown(message.content)}</div>
            {#if message.role === 'assistant'}
              <button
                class="copy-response"
                type="button"
                onclick={() => copyMessage(message)}
                aria-label="Copy response"
              >
                {copiedMessageId === message.id ? 'Copied' : 'Copy'}
              </button>
            {/if}
          </div>
        {/each}
        {#if transcriptNote && selectedSessionId && messages.length > 0}
          <p class="transcript-note" role="status">{transcriptNote}</p>
        {/if}
        {#if sending}
          <div class="message assistant pending">
            <span class="message-role">{provider === 'codex' ? 'Codex' : 'Claude'}</span>
            <div class="thinking">
              <span></span><span></span><span></span> Working on your post…
            </div>
          </div>
        {/if}
      </div>

      <form
        class="composer"
        onsubmit={(event) => {
          event.preventDefault();
          submitMessage();
        }}
      >
        <textarea
          bind:value={draft}
          bind:this={composerEl}
          aria-label="Message the AI assistant"
          placeholder="Write a question about this post…"
          disabled={sending}
          oninput={resizeComposer}
          onkeydown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault();
              submitMessage();
            }
          }}></textarea>
        <div class="composer-footer">
          <span
            >{sending
              ? 'Response usually takes a moment'
              : 'Enter to send · Shift+Enter for a new line'}</span
          >
          <button type="submit" disabled={!draft.trim() || sending} aria-label="Send message">
            {#if sending}…{:else}<Icon name="send" size={14} />{/if}
          </button>
        </div>
      </form>
    {:else}
      <div class="empty-placeholder">
        <span class="empty-placeholder-icon"><Icon name="ask-ai" size={20} /></span>
        <p>Select a post to start a conversation about it.</p>
      </div>
    {/if}
  {:else if item}
    <div class="history-heading">
      <div>
        <h3>Conversation history</h3>
        <p>Codex and Claude sessions for this post</p>
      </div>
    </div>
    <div class="history-list">
      {#if loadingSessions}
        <p class="subtle">Loading history…</p>
      {:else if sessions.length === 0}
        <div class="empty-placeholder">
          <span class="empty-placeholder-icon"><Icon name="ask-ai" size={20} /></span>
          <p>No AI conversations for this post yet.</p>
        </div>
      {:else}
        {#each sessions as session (session.id)}
          <div class="history-row" class:current={selectedSessionId === session.id}>
            <button
              class="history-open"
              type="button"
              disabled={sending}
              onclick={() => openSession(session)}
            >
              <span class="history-provider"
                >{session.provider === 'codex' ? 'Codex' : 'Claude'}</span
              >
              <span class="history-title">{session.title}</span>
              <time>{formatTime(session.updatedAt)}</time>
            </button>
            {#if confirmingRemoveId === session.id}
              <div class="remove-confirm">
                <button type="button" class="remove-cancel" onclick={cancelRemoveSession}
                  >Cancel</button
                >
                <button
                  type="button"
                  class="remove-confirm-btn"
                  onclick={() => removeSession(session)}>Remove</button
                >
              </div>
            {:else}
              <button
                class="remove-session"
                type="button"
                disabled={sending}
                aria-label="Remove from Aequo history"
                onclick={() => requestRemoveSession(session)}
              >
                <Icon name="close" size={13} />
              </button>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
    <p class="history-note">
      Removing a session here does not delete it from the CLI's own history.
    </p>
  {:else}
    <div class="empty-placeholder">
      <span class="empty-placeholder-icon"><Icon name="ask-ai" size={20} /></span>
      <p>Select a post to view its AI session history.</p>
    </div>
  {/if}

  {#if error}
    <div class="error-card" role="alert">
      <span class="error-mark"><Icon name="error" size={12} /></span>
      <div class="error-copy">
        <strong>Couldn’t complete that</strong>
        <p>{error}</p>
      </div>
      {#if lastPrompt && item && !sending}<button
          type="button"
          onclick={() => submitMessage(lastPrompt)}>Retry</button
        >{/if}
      <button
        class="dismiss-error"
        type="button"
        aria-label="Dismiss error"
        onclick={() => (error = null)}><Icon name="close" size={13} /></button
      >
    </div>
  {/if}
  <footer class="panel-footer">
    <span class="footer-dot"></span> Runs through your signed-in local CLI
    <span class="footer-divider">·</span> No API key in Aequo
  </footer>
</aside>

<style>
  .ai-panel {
    display: flex;
    min-width: 0;
    height: 100%;
    flex-direction: column;
    border-left: 1px solid var(--color-border);
    background: var(--color-bg);
    color: var(--color-text);
  }

  .ai-panel > :global(.empty-placeholder) {
    flex: 1;
    min-height: 0;
  }

  .panel-header {
    padding: 14px 14px 0;
    border-bottom: 1px solid var(--color-border);
  }

  .panel-title {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 11px;
  }

  .spark {
    display: inline-flex;
    align-items: center;
    color: var(--color-accent);
  }

  h2 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
  }

  .local-status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-left: auto;
    padding: 3px 7px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-pill);
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }

  .collapse-panel {
    display: grid;
    width: 24px;
    height: 24px;
    margin-left: 6px;
    flex-shrink: 0;
    place-items: center;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    cursor: pointer;
  }

  .collapse-panel:hover {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .local-status i,
  .footer-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-success);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-success) 16%, transparent);
  }

  .tabs {
    display: flex;
    gap: 16px;
  }

  .tabs button {
    padding: 0 1px 9px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--color-text-tertiary);
    font: inherit;
    font-size: var(--text-body);
    cursor: pointer;
  }

  .tabs button.active {
    border-bottom-color: var(--color-accent);
    color: var(--color-text);
    font-weight: var(--weight-medium);
  }

  .tabs button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .provider-picker {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 8px 12px 10px;
    background: var(--color-bg);
  }

  .provider-row {
    padding: 8px 13px 0;
  }

  .provider-label {
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    font-weight: var(--weight-bold);
    letter-spacing: 0.09em;
    border-bottom: 1px solid var(--color-border);
  }

  .provider-picker button {
    min-height: 32px;
    padding: 6px 11px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  .provider-picker button.active {
    border-color: color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
    background: color-mix(in srgb, var(--color-accent) 8%, var(--color-bg));
    color: var(--color-text);
    font-weight: var(--weight-medium);
  }

  .provider-picker .new-chat {
    margin-left: auto;
    color: var(--color-accent);
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .post-context {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 10px 12px;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
  }

  .context-copy {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 3px;
  }

  .context-label {
    flex-shrink: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    font-weight: var(--weight-bold);
    letter-spacing: 0.06em;
  }

  .context-title {
    overflow: hidden;
    color: var(--color-text-secondary);
    color: var(--color-text);
    font-size: var(--text-small);
    font-weight: var(--weight-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .context-link {
    display: grid;
    width: 27px;
    height: 27px;
    flex: 0 0 auto;
    place-items: center;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text-secondary);
    text-decoration: none;
  }

  .context-link:hover {
    color: var(--color-accent);
    background: var(--color-bg);
  }

  .conversation-state {
    flex: 0 0 auto;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }

  .chat-messages,
  .history-list {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    padding: 12px;
  }

  .history-list {
    padding: 14px;
  }

  .chat-empty {
    display: flex;
    min-height: 100%;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 20px 8px;
    color: var(--color-text-secondary);
    text-align: center;
  }

  .empty-icon {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    border: 1px solid color-mix(in srgb, var(--color-accent) 24%, var(--color-border));
    border-radius: var(--radius-xl);
    background: color-mix(in srgb, var(--color-accent) 9%, var(--color-bg));
    color: var(--color-accent);
  }

  .chat-empty h3 {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
  }

  .chat-empty p {
    max-width: 240px;
    margin: 0;
    font-size: var(--text-body);
    line-height: 1.5;
  }

  .suggestions {
    display: flex;
    width: 100%;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
    margin-top: 4px;
  }

  .suggestions button {
    padding: 6px 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-pill);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
    transition:
      border-color 120ms ease,
      color 120ms ease,
      transform 120ms ease;
  }

  .suggestions button:hover:not(:disabled) {
    transform: translateY(-1px);
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .summarize {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 36px;
    padding: 8px 12px;
    border: 0;
    border-radius: var(--radius-md);
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
    font: inherit;
    font-size: var(--text-body);
    font-weight: var(--weight-medium);
    cursor: pointer;
  }

  .summarize:hover:not(:disabled) {
    filter: brightness(1.05);
  }

  .hint,
  .subtle {
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .transcript-empty,
  .transcript-note {
    margin: 8px 4px 14px;
    padding: 9px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    line-height: 1.45;
  }

  .transcript-note {
    margin-top: 14px;
  }

  .message {
    position: relative;
    margin-bottom: 13px;
    padding: 10px 11px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg-secondary);
  }

  .message:last-child {
    margin-bottom: 0;
  }

  .message.user {
    border-color: color-mix(in srgb, var(--color-accent) 24%, var(--color-border));
    background: var(--color-selection-bg);
  }

  .message.user .message-role {
    color: var(--color-accent);
  }

  .copy-response {
    margin-top: 8px;
    padding: 3px 6px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    font: inherit;
    font-size: var(--text-caption);
    cursor: pointer;
  }

  .copy-response:hover {
    border-color: var(--color-border);
    background: var(--color-bg);
    color: var(--color-text);
  }

  .message-role {
    display: block;
    margin-bottom: 5px;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    font-weight: var(--weight-bold);
  }

  .message-content {
    color: var(--color-text);
    font-size: var(--text-body);
    line-height: 1.55;
    overflow-wrap: anywhere;
  }

  .message-content :global(p) {
    margin: 0 0 0.75em;
    white-space: normal;
  }

  .message-content :global(p:last-child) {
    margin-bottom: 0;
  }

  .message-content :global(h1),
  .message-content :global(h2),
  .message-content :global(h3),
  .message-content :global(h4) {
    margin: 1em 0 0.45em;
    line-height: 1.3;
  }

  .message-content :global(h1) {
    font-size: var(--text-title);
  }
  .message-content :global(h2) {
    font-size: var(--text-title);
  }
  .message-content :global(h3),
  .message-content :global(h4) {
    font-size: var(--text-body);
  }

  .message-content :global(ul),
  .message-content :global(ol) {
    margin: 0.5em 0 0.8em;
    padding-left: 1.4em;
  }

  .message-content :global(li + li) {
    margin-top: 0.3em;
  }

  .message-content :global(a) {
    color: var(--color-accent);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--color-accent) 45%, transparent);
    text-underline-offset: 2px;
  }

  .message-content :global(code) {
    padding: 0.12em 0.35em;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-xs);
    background: color-mix(in srgb, var(--color-bg) 72%, var(--color-accent) 8%);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--text-small);
  }

  .message-content :global(pre) {
    overflow: auto;
    margin: 0.75em 0;
    padding: 10px 11px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg);
    line-height: 1.5;
    tab-size: 2;
  }

  .message-content :global(pre code) {
    display: block;
    min-width: max-content;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font-size: var(--text-small);
    white-space: pre;
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
    background: var(--color-text-tertiary);
    animation: pulse 1.1s infinite ease-in-out;
  }

  .thinking span:nth-child(2) {
    animation-delay: 0.15s;
  }
  .thinking span:nth-child(3) {
    animation-delay: 0.3s;
    margin-right: 4px;
  }

  @keyframes pulse {
    0%,
    60%,
    100% {
      opacity: 0.35;
      transform: translateY(0);
    }
    30% {
      opacity: 1;
      transform: translateY(-2px);
    }
  }

  .pending {
    border-style: dashed;
    background: transparent;
  }

  .composer {
    margin: 10px;
    padding: 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
  }

  .composer:focus-within {
    border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 10%, transparent);
  }

  .composer textarea {
    display: block;
    width: 100%;
    min-height: 38px;
    max-height: 180px;
    resize: none;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    line-height: 1.45;
  }

  .composer textarea::placeholder {
    color: var(--color-text-tertiary);
  }

  .composer-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 6px;
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
  }

  .composer-footer button {
    display: grid;
    width: 32px;
    height: 32px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-lg);
    background: var(--color-accent-solid);
    color: var(--color-on-accent);
    font: inherit;
    font-size: var(--text-small);
    cursor: pointer;
  }

  .composer-footer button:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .history-heading {
    padding: 13px 13px 0;
  }

  .history-heading h3 {
    margin: 0 0 3px;
    font-size: var(--text-title);
    font-weight: var(--weight-bold);
  }

  .history-heading p {
    margin: 0;
    color: var(--color-text-tertiary);
    font-size: var(--text-body);
  }

  .history-row {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-bottom: 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg);
    transition:
      border-color 120ms ease,
      background 120ms ease;
  }

  .history-row:last-child {
    margin-bottom: 0;
  }

  .history-row:hover {
    border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
    background: var(--color-bg-secondary);
  }

  .history-row.current {
    border-color: var(--color-accent);
  }

  .history-open {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 11px 10px;
    border: 0;
    border-radius: inherit;
    background: transparent;
    color: var(--color-text);
    text-align: left;
    cursor: pointer;
  }

  .history-provider {
    display: inline-flex;
    align-items: center;
    min-height: 19px;
    padding: 2px 7px;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--color-accent) 12%, var(--color-bg));
    color: var(--color-accent);
    font-size: var(--text-small);
    font-weight: var(--weight-bold);
  }

  .history-title {
    width: 100%;
    overflow: hidden;
    font-size: var(--text-body);
    line-height: 1.35;
    font-weight: var(--weight-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .history-row time {
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
  }

  .remove-session {
    display: grid;
    flex-shrink: 0;
    margin-right: 5px;
    width: 28px;
    height: 28px;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    font: inherit;
    cursor: pointer;
  }

  .remove-session:hover {
    background: var(--color-hover-bg);
    color: var(--color-error);
  }

  .remove-confirm {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 4px;
    margin-right: 5px;
  }

  .remove-confirm button {
    min-height: 26px;
    padding: 3px 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font: inherit;
    font-size: var(--text-caption);
    cursor: pointer;
  }

  .remove-cancel:hover {
    background: var(--color-hover-bg);
  }

  .remove-confirm-btn {
    border-color: var(--color-error);
    background: var(--color-error);
    color: var(--color-on-error);
  }

  .remove-confirm-btn:hover {
    filter: brightness(0.92);
  }

  .history-open:focus-visible,
  .remove-session:focus-visible,
  .provider-picker button:focus-visible,
  .tabs button:focus-visible,
  .composer-footer button:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .thinking span {
      animation: none;
    }
    .history-row {
      transition: none;
    }
  }

  .history-note {
    margin: 0 12px 10px;
    color: var(--color-text-tertiary);
    font-size: var(--text-small);
    line-height: 1.5;
  }

  /* Base error-card look lives in the shared global (src/routes/+layout.svelte);
     only the panel-specific inset margin is overridden here. */
  :global(.ai-panel .error-card) {
    margin: 0 10px 8px;
  }

  .panel-footer {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 11px;
    border-top: 1px solid var(--color-border);
    color: var(--color-text-tertiary);
    font-size: var(--text-caption);
    line-height: 1.35;
  }

  .footer-dot {
    width: 5px;
    height: 5px;
    flex: 0 0 auto;
  }
  .footer-divider {
    opacity: 0.55;
  }

  @media (max-width: 380px) {
    .panel-footer {
      flex-wrap: wrap;
    }
    .conversation-state {
      display: none;
    }
    .composer-footer span {
      max-width: 220px;
    }
  }
</style>
