import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { AiStreamEvent, Item } from '$lib/types';

const api = vi.hoisted(() => ({
  startAiMessage: vi.fn(),
  cancelAiMessage: vi.fn(),
  listAiSessions: vi.fn(),
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

const item: Item = {
  id: 7,
  feedId: 1,
  guidHash: 'g',
  title: 'A post',
  link: 'https://blog.example/a',
  summary: null,
  fullContent: null,
  publishedAt: 0,
  fetchedAt: 0,
  isRead: true,
  readAt: null,
  isSaved: false,
};

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
    api.startAiMessage.mockResolvedValue({ requestId: 'r1', sessionId: 5 });
    api.loadAiTranscript.mockResolvedValue({ available: false, messages: [], note: null });
    api.cancelAiMessage.mockResolvedValue(undefined);
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
    expect(screen.getByText('Hello there')).toBeTruthy();

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

  it('does not send while an IME is composing', async () => {
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    const composer = screen.getByLabelText('Message the AI assistant');
    await fireEvent.input(composer, { target: { value: '안녕' } });
    await fireEvent.keyDown(composer, { key: 'Enter', isComposing: true });
    expect(api.startAiMessage).not.toHaveBeenCalled();
  });

  it('switching provider starts a new chat and offers to go back', async () => {
    const session = {
      id: 5,
      itemId: 7,
      provider: 'codex' as const,
      providerSessionId: 't',
      title: 'Earlier chat',
      createdAt: 0,
      updatedAt: 0,
    };
    api.listAiSessions.mockImplementation(async (_id: number, provider: string) =>
      provider === 'codex' ? [session] : [],
    );
    api.loadAiTranscript.mockResolvedValue({
      available: true,
      messages: [{ id: 1, sessionId: 5, role: 'assistant', content: 'Old answer', createdAt: 0 }],
      note: null,
    });
    render(AiPanel, { props: { item, onCollapse: vi.fn() } });
    await settle();
    await fireEvent.click(screen.getByRole('button', { name: /Earlier chat/ }));
    await settle();
    expect(screen.getByText('Old answer')).toBeTruthy();

    await fireEvent.change(screen.getByLabelText('AI assistant', { selector: 'select' }), {
      target: { value: 'claude' },
    });
    expect(screen.queryByText('Old answer')).toBeNull();
    expect(localStorage.getItem('aequo:aiProvider')).toBe('claude');

    await fireEvent.click(screen.getByRole('button', { name: 'Go back' }));
    await settle();
    expect(screen.getByText('Old answer')).toBeTruthy();
  });
});
