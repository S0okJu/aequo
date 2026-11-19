import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { AiSession, AiStreamEvent, Item } from '$lib/types';

const api = vi.hoisted(() => ({
  startAiMessage: vi.fn(),
  cancelAiMessage: vi.fn(),
  listAiSessions: vi.fn(),
  listItems: vi.fn(),
  listAllAiSessions: vi.fn(),
  addAiContextItems: vi.fn(),
  removeAiContextItem: vi.fn(),
  loadAiTranscript: vi.fn(),
  deleteAiSession: vi.fn(),
}));
vi.mock('$lib/api', () => api);

const events = vi.hoisted(() => ({ handler: null as null | ((e: { payload: unknown }) => void) }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_name: string, handler: (e: { payload: unknown }) => void) => {
    events.handler = handler;
    return () => {};
  }),
}));

import AiPanel from './AiPanel.svelte';
import { managedCache } from '$lib/cacheBudget';

const item: Item = {
  id: 7,
  feedId: 1,
  guidHash: 'g',
  title: 'A post',
  link: 'https://blog.example/a',
  preview: null,
  publishedAt: 0,
  fetchedAt: 0,
  isRead: true,
  readAt: null,
  isSaved: false,
};

/** A second post, for the "reader moved on" cases. */
const otherItem: Item = { ...item, id: 8, title: 'Another post', link: 'https://blog.example/b' };

function session(overrides: Partial<AiSession> = {}): AiSession {
  return {
    id: 5,
    itemId: 7,
    provider: 'codex',
    providerSessionId: 't',
    title: 'Earlier chat',
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

/** `listAiSessions` is called once per provider; serve each its own list. */
function serveSessions(byProvider: { codex?: AiSession[]; claude?: AiSession[] }) {
  api.listAiSessions.mockImplementation(async (_id: number, provider: string) =>
    provider === 'claude' ? (byProvider.claude ?? []) : (byProvider.codex ?? []),
  );
}

/**
 * The resume divider. Scoped because its "New conversation" button shares an
 * accessible name with the header's icon button.
 */
function resumedNote(): HTMLElement {
  const note = screen.getByText(/Earlier conversation/).closest('p');
  if (!note) throw new Error('resume divider not rendered');
  return note;
}

function emit(event: Partial<AiStreamEvent>) {
  events.handler?.({ payload: { requestId: 'r1', sessionId: 5, text: '', ...event } });
}

async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
}

describe('AiPanel', () => {
  beforeEach(() => {
    localStorage.clear();
    for (const fn of Object.values(api)) fn.mockReset();
    api.listAiSessions.mockResolvedValue([]);
    api.listAllAiSessions.mockResolvedValue([]);
    api.startAiMessage.mockResolvedValue({ requestId: 'r1', sessionId: 5 });
    api.loadAiTranscript.mockResolvedValue({ available: false, messages: [], note: null });
    api.cancelAiMessage.mockResolvedValue(undefined);
  });

  it('summarizes the requested article once in a fresh conversation instead of resuming history', async () => {
    serveSessions({ codex: [session()] });
    const onBusyChange = vi.fn();
    const request = { key: 1, items: [otherItem], action: 'summarize' as const };
    const { rerender } = render(AiPanel, {
      props: { item: otherItem, contextRequest: request, onCollapse: vi.fn(), onBusyChange },
    });
    await waitFor(() =>
      expect(api.startAiMessage).toHaveBeenCalledWith(
        [8],
        'codex',
        null,
        'Summarize this post: the main idea and the key points.',
        null,
      ),
    );
    expect(api.loadAiTranscript).not.toHaveBeenCalled();
    expect(screen.getByText('Writing an answer…')).toBeTruthy();
    expect(onBusyChange).toHaveBeenLastCalledWith(true);
    emit({ kind: 'done', text: 'Summary of B' });
    await settle();
    await rerender({ contextRequest: { ...request } });
    await settle();
    expect(api.startAiMessage).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Summary of B')).toBeTruthy();
    expect(onBusyChange).toHaveBeenLastCalledWith(false);
  });

  it('opens and closes reference management with Escape without changing the conversation', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();
    const trigger = screen.getByRole('button', { name: /Using 1 article/ });
    await fireEvent.click(trigger);
    const dialog = screen.getByRole('dialog', { name: 'View and manage reference articles' });
    expect(within(dialog).getByRole('button', { name: '[1] A post' })).toBeTruthy();
    await fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(api.startAiMessage).not.toHaveBeenCalled();
  });

  it('keeps a main-screen conversation independent of articles opened in the reader', async () => {
    const { rerender } = render(AiPanel, {
      props: { item: null, standalone: true, onCollapse: vi.fn() },
    });
    await rerender({ item });
    await settle();
    expect(api.loadAiTranscript).not.toHaveBeenCalled();
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'A general question' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await waitFor(() =>
      expect(api.startAiMessage).toHaveBeenCalledWith(
        [],
        'codex',
        null,
        'A general question',
        null,
      ),
    );
    emit({ kind: 'done', text: 'General answer' });
    await settle();
    await rerender({ item: otherItem });
    expect(screen.getByText('General answer')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Add to this chat' })).toBeNull();
    // "New conversation" lives inside the history popover now.
    await fireEvent.click(screen.getByRole('button', { name: 'Conversation history' }));
    await fireEvent.click(screen.getByRole('button', { name: 'New conversation' }));
    await fireEvent.input(composer, { target: { value: 'Another general question' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await waitFor(() =>
      expect(api.startAiMessage).toHaveBeenLastCalledWith(
        [],
        'codex',
        null,
        'Another general question',
        null,
      ),
    );
  });

  it('selects @this before sending and keeps the selected article when the reader moves on', async () => {
    api.listItems.mockResolvedValue([]);
    const { rerender } = render(AiPanel, {
      props: { item, standalone: true, onCollapse: vi.fn() },
    });
    const composer = screen.getByLabelText('Message the AI assistant') as HTMLTextAreaElement;
    await fireEvent.input(composer, { target: { value: 'Summarize @this' } });
    await screen.findByRole('option', { name: /@this/ });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await waitFor(() => expect(composer.value).toBe('Summarize @A post '));
    expect(api.startAiMessage).not.toHaveBeenCalled();
    await rerender({ item: otherItem });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await waitFor(() =>
      expect(api.startAiMessage).toHaveBeenCalledWith(
        [7],
        'codex',
        null,
        'Summarize @A post',
        null,
      ),
    );
  });

  it('finds articles outside the reader, adds multiple references, and keeps both mentions visible in the question', async () => {
    api.listItems.mockResolvedValue([otherItem]);
    const { rerender } = render(AiPanel, {
      props: { item: null, standalone: true, onCollapse: vi.fn() },
    });
    const composer = screen.getByLabelText('Message the AI assistant') as HTMLTextAreaElement;
    await fireEvent.input(composer, { target: { value: 'Compare @Another' } });
    const result = await screen.findByRole('option', { name: /Another post/ });
    await fireEvent.click(result);
    await waitFor(() => expect(composer.value).toBe('Compare @Another post '));
    expect(api.listItems).toHaveBeenCalledWith({ query: 'Another' }, 30);
    await rerender({ item });
    await fireEvent.input(composer, { target: { value: `${composer.value}and @this` } });
    await screen.findByRole('option', { name: /@this/ });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await waitFor(() => expect(composer.value).toBe('Compare @Another post and @A post '));
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await waitFor(() =>
      expect(api.startAiMessage).toHaveBeenCalledWith(
        [8, 7],
        'codex',
        null,
        'Compare @Another post and @A post',
        null,
      ),
    );
  });

  it('does not select a mention or send a message while Korean input is composing', async () => {
    api.listItems.mockResolvedValue([]);
    render(AiPanel, { props: { item, standalone: true, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant') as HTMLTextAreaElement;
    await fireEvent.input(composer, { target: { value: '@this' } });
    await screen.findByRole('option', { name: /@this/ });
    await fireEvent.keyDown(composer, { key: 'Enter', isComposing: true });
    expect(composer.value).toBe('@this');
    expect(api.startAiMessage).not.toHaveBeenCalled();
    await fireEvent.keyDown(composer, { key: 'Escape' });
    expect(screen.queryByRole('listbox')).toBeNull();
    expect(composer.value).toBe('@this');
  });

  it('resolves a fully typed @this on send and prevents sending it without an open article', async () => {
    const { rerender } = render(AiPanel, {
      props: { item: null, standalone: true, onCollapse: vi.fn() },
    });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'Explain @this please ' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    expect(api.startAiMessage).not.toHaveBeenCalled();
    expect(screen.getByText(/Open an article to use @this/)).toBeTruthy();
    await rerender({ item });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await waitFor(() =>
      expect(api.startAiMessage).toHaveBeenCalledWith(
        [7],
        'codex',
        null,
        'Explain @this please',
        null,
      ),
    );
  });

  it('keeps the question and mention token when adding to a saved conversation fails', async () => {
    serveSessions({ codex: [session()] });
    api.loadAiTranscript.mockResolvedValue({ available: true, messages: [], note: null });
    api.listItems.mockResolvedValue([otherItem]);
    api.addAiContextItems.mockRejectedValue(new Error('Could not attach article'));
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await waitFor(() => expect(api.loadAiTranscript).toHaveBeenCalled());
    await settle();
    const composer = screen.getByLabelText('Message the AI assistant') as HTMLTextAreaElement;
    await fireEvent.input(composer, { target: { value: 'Compare @Another' } });
    await fireEvent.click(await screen.findByRole('option', { name: /Another post/ }));
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toContain('Could not attach article'),
    );
    expect(composer.value).toBe('Compare @Another');
    expect(api.startAiMessage).not.toHaveBeenCalled();
  });

  it('streams the answer as it arrives, including events that beat the request id', async () => {
    let resolveStart: (value: { requestId: string; sessionId: number }) => void = () => {};
    api.startAiMessage.mockReturnValue(new Promise((resolve) => (resolveStart = resolve)));
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });

    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'What is new?' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });

    emit({ kind: 'delta', text: 'Hello' }); // before start_ai_message returned
    resolveStart({ requestId: 'r1', sessionId: 5 });
    await settle();
    emit({ kind: 'delta', text: ' there' });
    await settle();
    await waitFor(() => expect(screen.getByText('Hello there')).toBeTruthy());

    emit({ kind: 'done', text: 'Hello there' });
    await settle();
    expect(api.loadAiTranscript).toHaveBeenCalledWith(5);
    expect(screen.getByText('What is new?')).toBeTruthy();
    expect(screen.getByText('Hello there')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Send message' })).toBeTruthy();
  });

  it('stops a running answer and keeps what streamed', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'Long question' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    emit({ kind: 'delta', text: 'Partial' });
    await settle();

    await fireEvent.click(screen.getByRole('button', { name: 'Stop response' }));
    expect(api.cancelAiMessage).toHaveBeenCalledWith('r1');
    emit({ kind: 'cancelled', text: 'Partial' });
    await settle();
    expect(screen.getByText('Partial')).toBeTruthy();
    expect(screen.getByText('Response stopped.')).toBeTruthy();
  });

  it('shows a friendlier title for a classified error, keeping the raw detail', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'What is new?' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    emit({ kind: 'error', text: 'rate limit exceeded, retry later', errorKind: 'rateLimit' });
    await settle();

    expect(screen.getByText('Usage limit reached')).toBeTruthy();
    expect(screen.getByText('rate limit exceeded, retry later')).toBeTruthy();
  });

  it('falls back to the generic error title when nothing matched', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'What is new?' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    emit({ kind: 'error', text: 'codex CLI exited with status 1', errorKind: 'unknown' });
    await settle();

    expect(screen.getByText("Couldn't complete that")).toBeTruthy();
    expect(screen.getByText('codex CLI exited with status 1')).toBeTruthy();
  });

  it('offers to continue after a stop, as a new turn over the same session', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'Long question' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: 'Stop response' }));
    emit({ kind: 'cancelled', text: 'Partial' });
    await settle();

    await fireEvent.click(screen.getByRole('button', { name: 'Continue writing' }));
    await waitFor(() =>
      expect(api.startAiMessage).toHaveBeenLastCalledWith(
        [7],
        'codex',
        5,
        'Please continue your previous answer.',
        null,
      ),
    );
    expect(screen.queryByText('Response stopped.')).toBeNull();
  });

  it('changes AI from the compact selector and disables it during a response', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();
    const picker = screen.getByRole('combobox', { name: 'Change AI' });
    await fireEvent.change(picker, { target: { value: 'claude' } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'Explain this' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    expect(api.startAiMessage).toHaveBeenCalledWith([7], 'claude', null, 'Explain this', null);
    // Provider selection is disabled until the response finishes.
    expect(picker).toHaveProperty('disabled', true);
    expect(screen.getByText('You can change AI after the response finishes.')).toBeTruthy();
    emit({ kind: 'done', text: 'Explained' });
    await settle();
    expect(picker).toHaveProperty('disabled', false);
    expect(picker).toHaveProperty('value', 'claude');
  });

  it('does not send while an IME is composing', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: '안녕' } });
    await fireEvent.keyDown(composer, { key: 'Enter', isComposing: true });
    expect(api.startAiMessage).not.toHaveBeenCalled();
  });

  it('switching provider starts a new chat and offers to go back', async () => {
    serveSessions({ codex: [session()] });
    api.loadAiTranscript.mockResolvedValue({
      available: true,
      messages: [{ id: 1, sessionId: 5, role: 'assistant', content: 'Old answer', createdAt: 0 }],
      note: null,
    });
    // The conversation is already open on mount: auto-resume replaces the
    // click on the "Earlier chat" entry this test used to need.
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();
    expect(screen.getByText('Old answer')).toBeTruthy();
    expect(screen.getByText(/Earlier conversation/)).toBeTruthy();

    await fireEvent.change(screen.getByRole('combobox', { name: 'Change AI' }), {
      target: { value: 'claude' },
    });
    // Switching mid-conversation asks for confirmation first (§D).
    expect(screen.getByText('Old answer')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Switch' }));
    expect(screen.queryByText('Old answer')).toBeNull();
    expect(localStorage.getItem('aequo:aiProvider')).toBe('claude');

    await fireEvent.click(screen.getByRole('button', { name: 'Go back' }));
    await settle();
    expect(screen.getByText('Old answer')).toBeTruthy();
  });

  it('cancelling a provider switch keeps the current conversation and selection', async () => {
    serveSessions({ codex: [session()] });
    api.loadAiTranscript.mockResolvedValue({
      available: true,
      messages: [{ id: 1, sessionId: 5, role: 'assistant', content: 'Old answer', createdAt: 0 }],
      note: null,
    });
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();

    await fireEvent.change(screen.getByRole('combobox', { name: 'Change AI' }), {
      target: { value: 'claude' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(screen.getByText('Old answer')).toBeTruthy();
    expect((screen.getByRole('combobox', { name: 'Change AI' }) as HTMLSelectElement).value).toBe(
      'codex',
    );
    expect(localStorage.getItem('aequo:aiProvider')).not.toBe('claude');
  });

  it('keeps the draft and quote reference when switching provider', async () => {
    render(AiPanel, {
      props: {
        item,
        pendingQuote: 'A quoted line',
        onCollapse: vi.fn(),
        onConsumedPendingQuote: vi.fn(),
      },
    });
    await settle();
    expect(screen.getByText('"A quoted line"')).toBeTruthy();

    const composer = screen.getByLabelText('Message the AI assistant') as HTMLTextAreaElement;
    await fireEvent.input(composer, { target: { value: `${composer.value}Draft text` } });

    await fireEvent.change(screen.getByRole('combobox', { name: 'Change AI' }), {
      target: { value: 'claude' },
    });
    await settle();

    expect((composer as HTMLTextAreaElement).value).toBe('"A quoted line" Draft text');
    expect(screen.getByText('"A quoted line"')).toBeTruthy();
  });

  it('reopens the most recent conversation when the post has one', async () => {
    serveSessions({ codex: [session({ id: 5, updatedAt: 10 }), session({ id: 4, updatedAt: 5 })] });
    api.loadAiTranscript.mockResolvedValue({
      available: true,
      messages: [{ id: 1, sessionId: 5, role: 'assistant', content: 'Old answer', createdAt: 0 }],
      note: null,
    });
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();

    expect(api.loadAiTranscript).toHaveBeenCalledWith(5);
    expect(screen.getByText('Old answer')).toBeTruthy();
    expect(within(resumedNote()).getByRole('button', { name: 'New conversation' })).toBeTruthy();
    // The ask-chips belong to an empty chat and must be out of the way.
    expect(screen.queryByRole('button', { name: 'Summarize' })).toBeNull();
  });

  it('starts empty when the post has no conversations', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();

    expect(api.loadAiTranscript).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Summarize' })).toBeTruthy();
    expect(screen.queryByText(/Earlier conversation/)).toBeNull();
  });

  it('keeps quoted selections usable without a permanent selection tutorial', async () => {
    const onConsumedPendingQuote = vi.fn();
    render(AiPanel, {
      props: { item, pendingQuote: 'Quoted', onCollapse: vi.fn(), onConsumedPendingQuote },
    });
    await settle();
    expect(screen.getByText('"Quoted"')).toBeTruthy();
    expect(onConsumedPendingQuote).toHaveBeenCalled();
    expect(screen.queryByText(/Select a sentence in the article/)).toBeNull();
    const composer = screen.getByLabelText('Message the AI assistant') as HTMLTextAreaElement;
    await fireEvent.input(composer, { target: { value: `${composer.value}Explain this quote` } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await waitFor(() =>
      expect(api.startAiMessage).toHaveBeenCalledWith(
        [7],
        'codex',
        null,
        '> Quoted\n\nExplain this quote',
        null,
      ),
    );
  });

  it('resumes the latest conversation across providers', async () => {
    serveSessions({ claude: [session({ provider: 'claude', title: 'Claude chat' })] });
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();

    expect(api.loadAiTranscript).toHaveBeenCalledWith(5);
    expect(screen.getByRole('combobox', { name: 'Change AI' })).toHaveProperty('value', 'claude');
  });

  it('keeps a started conversation when the reader moves to another post', async () => {
    const { rerender } = render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'About this one' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    emit({ kind: 'done', text: 'An answer' });
    await settle();

    await rerender({ item: otherItem, onCollapse: vi.fn() });
    await settle();

    // The transcript survives, and the bar names the post now on screen.
    expect(screen.getByText('An answer')).toBeTruthy();
    expect(screen.getByText(/Another post/)).toBeTruthy();

    // A follow-up still belongs to the post the conversation is about.
    await fireEvent.input(composer, { target: { value: 'And one more thing' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    expect(api.startAiMessage).toHaveBeenLastCalledWith(
      [7],
      'codex',
      5,
      'And one more thing',
      null,
    );
  });

  it('follows the open post while the conversation is still empty', async () => {
    const { rerender } = render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();

    await rerender({ item: otherItem, onCollapse: vi.fn() });
    await settle();

    expect(api.listAiSessions).toHaveBeenCalledWith(8, 'codex');
    expect(screen.queryByText(/Open in the reader/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Summarize' })).toBeTruthy();
  });

  it('moves the conversation to the open post on request', async () => {
    const { rerender } = render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'About this one' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    emit({ kind: 'done', text: 'An answer' });
    await settle();

    serveSessions({ codex: [session({ id: 9, itemId: 8, title: 'Old B conversation' })] });
    await rerender({ item: otherItem, onCollapse: vi.fn() });
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: 'New chat here' }));
    await settle();

    expect(screen.queryByText('An answer')).toBeNull();
    await fireEvent.input(composer, { target: { value: 'About the new one' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    expect(api.startAiMessage).toHaveBeenLastCalledWith(
      [8],
      'codex',
      null,
      'About the new one',
      null,
    );
  });

  it('can still stop a run after the reader moved to another post', async () => {
    const { rerender } = render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'Long question' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    await settle();
    emit({ kind: 'delta', text: 'Partial' });
    await settle();

    await rerender({ item: otherItem, onCollapse: vi.fn() });
    await settle();

    await fireEvent.click(screen.getByRole('button', { name: 'Stop response' }));
    expect(api.cancelAiMessage).toHaveBeenCalledWith('r1');
    emit({ kind: 'cancelled', text: 'Partial' });
    await settle();
    expect(screen.getByText('Partial')).toBeTruthy();
  });

  it('keeps a resumed transcript out of a chat the user has since emptied', async () => {
    serveSessions({ codex: [session()] });
    let resolveTranscript: (value: unknown) => void = () => {};
    api.loadAiTranscript.mockReturnValue(new Promise((resolve) => (resolveTranscript = resolve)));
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();

    // Only the history popover's option exists yet: the divider needs loaded
    // messages, and the toolbar button moved into that popover.
    await fireEvent.click(screen.getByRole('button', { name: 'Conversation history' }));
    await fireEvent.click(screen.getByRole('button', { name: 'New conversation' }));
    // The resume's transcript lands after the new chat started: it must lose.
    resolveTranscript({
      available: true,
      messages: [{ id: 1, sessionId: 5, role: 'assistant', content: 'Old answer', createdAt: 0 }],
      note: null,
    });
    await settle();

    expect(screen.queryByText('Old answer')).toBeNull();
    expect(screen.getByRole('button', { name: 'Summarize' })).toBeTruthy();
  });
});

describe('AiPanel transcript paging and cleanup', () => {
  beforeEach(() => {
    localStorage.clear();
    for (const fn of Object.values(api)) fn.mockReset();
    api.listAiSessions.mockImplementation(async (_id: number, provider: string) =>
      provider === 'codex' ? [session()] : [],
    );
    managedCache.setLimit(16 * 1024 * 1024);
  });
  it('replaces older/newer pages and releases all transcript bytes on unmount', async () => {
    const msg = (id: number) => ({
      id,
      sessionId: 5,
      role: 'assistant',
      content: `page answer ${id}`,
      createdAt: 0,
    });
    api.loadAiTranscript.mockResolvedValueOnce({
      available: true,
      messages: [msg(21)],
      note: null,
      hasOlder: true,
      hasNewer: false,
    });
    const { unmount } = render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await waitFor(() => expect(screen.getByText('page answer 21')).toBeTruthy());
    expect(managedCache.bytes).toBeGreaterThan(0);
    api.loadAiTranscript.mockResolvedValueOnce({
      available: true,
      messages: [msg(1)],
      note: null,
      hasOlder: false,
      hasNewer: true,
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Previous messages' }));
    await waitFor(() => expect(screen.getByText('page answer 1')).toBeTruthy());
    expect(api.loadAiTranscript).toHaveBeenLastCalledWith(5, { before: 21 });
    expect(screen.queryByText('page answer 21')).toBeNull();
    api.loadAiTranscript.mockResolvedValueOnce({
      available: true,
      messages: [msg(21)],
      note: null,
      hasOlder: true,
      hasNewer: false,
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Next messages' }));
    await waitFor(() => expect(screen.getByText('page answer 21')).toBeTruthy());
    expect(api.loadAiTranscript).toHaveBeenLastCalledWith(5, { after: 1 });
    unmount();
    expect(managedCache.bytes).toBe(0);
  });
  it('ignores a page that returns after a new conversation starts', async () => {
    api.loadAiTranscript.mockResolvedValueOnce({
      available: true,
      messages: [{ id: 21, sessionId: 5, role: 'assistant', content: 'latest page', createdAt: 0 }],
      note: null,
      hasOlder: true,
      hasNewer: false,
    });
    const { unmount } = render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await waitFor(() => expect(screen.getByText('latest page')).toBeTruthy());
    let resolvePage: (page: unknown) => void = () => {};
    api.loadAiTranscript.mockReturnValueOnce(
      new Promise((resolve) => {
        resolvePage = resolve;
      }),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Previous messages' }));
    await fireEvent.click(screen.getAllByRole('button', { name: 'New conversation' })[0]);
    resolvePage({
      available: true,
      messages: [
        { id: 1, sessionId: 5, role: 'assistant', content: 'late old page', createdAt: 0 },
      ],
      note: null,
      hasOlder: false,
      hasNewer: true,
    });
    await settle();
    expect(screen.queryByText('late old page')).toBeNull();
    expect(managedCache.bytes).toBe(0);
    unmount();
  });
  it('bounds events received before the request id and stops the oversized run', async () => {
    api.listAiSessions.mockResolvedValue([]);
    api.listAllAiSessions.mockResolvedValue([]);
    let resolveStart: (value: { requestId: string; sessionId: number }) => void = () => {};
    api.startAiMessage.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      }),
    );
    api.cancelAiMessage.mockResolvedValue(undefined);
    const { unmount } = render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await fireEvent.input(screen.getByLabelText('Message the AI assistant'), {
      target: { value: 'Question' },
    });
    await fireEvent.keyDown(screen.getByLabelText('Message the AI assistant'), { key: 'Enter' });
    for (let i = 0; i < 257; i++) emit({ kind: 'delta', text: 'x' });
    resolveStart({ requestId: 'r1', sessionId: 5 });
    await waitFor(() => expect(api.cancelAiMessage).toHaveBeenCalledWith('r1'));
    expect(screen.getByRole('alert').textContent).toContain('display limit');
    expect(screen.queryByRole('button', { name: 'Stop response' })).toBeNull();
    unmount();
  });
  it('does not retain a transcript response that returns after the panel closes', async () => {
    let resolveTranscript: (value: unknown) => void = () => {};
    api.loadAiTranscript.mockReturnValue(
      new Promise((resolve) => {
        resolveTranscript = resolve;
      }),
    );
    const { unmount } = render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await waitFor(() => expect(api.loadAiTranscript).toHaveBeenCalled());
    unmount();
    resolveTranscript({
      available: true,
      messages: [{ id: 1, sessionId: 5, role: 'assistant', content: 'late', createdAt: 0 }],
      note: null,
      hasOlder: false,
      hasNewer: false,
    });
    await settle();
    expect(managedCache.bytes).toBe(0);
  });
});

describe('multi-article conversations', () => {
  beforeEach(() => {
    localStorage.clear();
    for (const fn of Object.values(api)) fn.mockReset();
    api.listAiSessions.mockResolvedValue([]);
    api.listAllAiSessions.mockResolvedValue([]);
    api.startAiMessage.mockResolvedValue({ requestId: 'r1', sessionId: 5 });
    api.loadAiTranscript.mockResolvedValue({ available: true, messages: [], note: null });
  });
  it('opens a frozen view without a reader and submits explicit IDs and scope', async () => {
    const { rerender } = render(AiPanel, {
      props: {
        item: null,
        onCollapse: vi.fn(),
        contextRequest: { key: 1, items: [item, otherItem], scopeLabel: 'Today · 2' },
      },
    });
    await settle();
    expect(screen.getByRole('button', { name: 'Group by topic' })).toBeTruthy();
    await rerender({ item: { ...item, id: 99 } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'Compare these' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    expect(api.startAiMessage).toHaveBeenCalledWith(
      [7, 8],
      'codex',
      null,
      'Compare these',
      'Today · 2',
    );
  });
  it('adds the open article to a resumed conversation and follows citation links', async () => {
    const contextItems = [{ itemId: 7, itemTitle: item.title, feedId: 1, position: 0 }];
    api.listAiSessions.mockImplementation(async (_: number, p: string) =>
      p === 'codex' ? [session({ contextItems })] : [],
    );
    api.loadAiTranscript.mockResolvedValue({
      available: true,
      messages: [{ id: 1, sessionId: 5, role: 'assistant', content: 'See [1]', createdAt: 0 }],
      note: null,
    });
    api.addAiContextItems.mockResolvedValue(
      session({
        contextItems: [
          ...contextItems,
          { itemId: 8, itemTitle: otherItem.title, feedId: 1, position: 1 },
        ],
      }),
    );
    const onOpenItem = vi.fn();
    const { rerender } = render(AiPanel, { props: { item, onOpenItem, onCollapse: vi.fn() } });
    await waitFor(() =>
      expect(screen.getByRole('link', { name: 'Source 1: A post' })).toBeTruthy(),
    );
    await fireEvent.click(screen.getByRole('link', { name: 'Source 1: A post' }));
    expect(onOpenItem).toHaveBeenCalledWith(7);
    await fireEvent.click(screen.getByRole('link', { name: '1 A post' }));
    expect(onOpenItem).toHaveBeenCalledTimes(2);
    await rerender({ item: otherItem });
    await fireEvent.click(screen.getByRole('button', { name: 'Add to this chat' }));
    await waitFor(() => expect(api.addAiContextItems).toHaveBeenCalledWith(5, [8]));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Add to this chat' })).toBeNull(),
    );
    expect(api.loadAiTranscript).toHaveBeenCalledTimes(1);
  });
  it('can remove the last article and continue without article context', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: /Using 1 article/ }));
    await fireEvent.click(screen.getByRole('button', { name: 'Remove A post from context' }));
    expect(screen.getByText('No article context')).toBeTruthy();
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: 'Hello' } });
    await fireEvent.keyDown(composer, { key: 'Enter' });
    expect(api.startAiMessage).toHaveBeenCalledWith([], 'codex', null, 'Hello', null);
  });
  it('loads and removes all-conversations history without a reader', async () => {
    api.listAllAiSessions.mockResolvedValue([
      session({ title: 'Multi article discussion', contextItems: [] }),
    ]);
    render(AiPanel, { props: { item: null, onCollapse: vi.fn() } });
    await fireEvent.click(screen.getByRole('button', { name: 'Conversation history' }));
    await fireEvent.click(screen.getByRole('radio', { name: 'All' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Multi article discussion/ })).toBeTruthy(),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Remove conversation' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: /Multi article discussion/ })).toBeNull(),
    );
    expect(api.deleteAiSession).toHaveBeenCalledWith(5);
  });
});

it('freezes a view at the first send and never includes later arrivals in follow-up turns', async () => {
  localStorage.clear();
  for (const fn of Object.values(api)) fn.mockReset();
  const newest = { ...item, id: 99, title: 'Just arrived' };
  api.listAiSessions.mockResolvedValue([]);
  api.listItems.mockResolvedValue([newest, item]);
  api.startAiMessage.mockResolvedValue({ requestId: 'r1', sessionId: 5 });
  api.loadAiTranscript.mockResolvedValue({ available: true, messages: [], note: null });
  render(AiPanel, {
    props: {
      item: null,
      onCollapse: vi.fn(),
      contextRequest: {
        key: 1,
        items: [item],
        scopeLabel: 'Today',
        scopeFilter: { unreadOnly: true },
      },
    },
  });
  await settle();
  const composer = screen.getByLabelText('Message the AI assistant');
  await fireEvent.input(composer, { target: { value: 'Summarize' } });
  await fireEvent.keyDown(composer, { key: 'Enter' });
  await waitFor(() =>
    expect(api.startAiMessage).toHaveBeenCalledWith([99, 7], 'codex', null, 'Summarize', 'Today'),
  );
  expect(api.listItems).toHaveBeenCalledWith({ unreadOnly: true }, 20);
  emit({ kind: 'done', text: 'Summary' });
  await settle();
  api.listItems.mockResolvedValue([{ ...item, id: 100 }]);
  await fireEvent.input(composer, { target: { value: 'More detail' } });
  await fireEvent.keyDown(composer, { key: 'Enter' });
  await waitFor(() =>
    expect(api.startAiMessage).toHaveBeenLastCalledWith(
      [99, 7],
      'codex',
      5,
      'More detail',
      'Today',
    ),
  );
  expect(api.listItems).toHaveBeenCalledTimes(1);
});
