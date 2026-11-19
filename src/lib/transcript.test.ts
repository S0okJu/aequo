import { describe, expect, it } from 'vitest';
import { CacheBudget, serializedBytes } from './cacheBudget';
import { TranscriptWindow } from './transcript.svelte';
import type { AiMessage, AiTranscript } from './types';

function message(id: number, content = `answer ${id}`): AiMessage {
  return { id, content, role: 'assistant', sessionId: 7, createdAt: 0 };
}
function page(messages: AiMessage[], hasOlder = false, hasNewer = false): AiTranscript {
  return { messages, hasOlder, hasNewer, available: true, note: null };
}

describe('TranscriptWindow', () => {
  it('replaces pages, shares bytes with other caches, and releases on close', () => {
    const cache = new CacheBudget(5000);
    const transcript = new TranscriptWindow(cache);
    const recent = Array.from({ length: 20 }, (_, i) => message(i + 81));
    transcript.accept(page(recent, true));
    expect(cache.bytes).toBe(serializedBytes(recent));
    transcript.accept(page([message(61), message(62)], true, true));
    expect(transcript.before).toBe(61);
    expect(transcript.after).toBe(62);
    expect(transcript.messages).toHaveLength(2);
    cache.hold('visible-list', 4900, (maximum) => maximum);
    expect(cache.bytes).toBeLessThanOrEqual(5000);
    expect(transcript.budgetLimited).toBe(true);
    expect(transcript.hasOlder).toBe(true);
    transcript.dispose();
    expect(cache.bytes).toBe(4900);
  });

  it('shrinks immediately when settings reduce bytes and can load the page again', () => {
    const cache = new CacheBudget(5000);
    const transcript = new TranscriptWindow(cache);
    const recent = Array.from({ length: 20 }, (_, i) => message(i + 1, '한😀'.repeat(10)));
    transcript.accept(page(recent));
    cache.setLimit(serializedBytes(recent.slice(-3)));
    expect(transcript.messages.map((message) => message.id)).toEqual([18, 19, 20]);
    expect(transcript.before).toBe(18);
    expect(transcript.budgetLimited).toBe(true);
    cache.setLimit(0);
    expect(transcript.messages).toHaveLength(0);
    expect(cache.bytes).toBe(0);
    cache.setLimit(5000);
    transcript.accept(page(recent));
    expect(transcript.messages).toHaveLength(20);
    expect(transcript.budgetLimited).toBe(false);
    transcript.dispose();
    expect(cache.bytes).toBe(0);
  });

  it('bounds fallback turns and never joins an old page across a missing interval', () => {
    const cache = new CacheBudget(10000);
    const transcript = new TranscriptWindow(cache);
    transcript.accept(
      page(
        Array.from({ length: 20 }, (_, i) => message(i + 1)),
        false,
        true,
      ),
    );
    transcript.appendLocal([message(-1, 'latest question'), message(-2, 'latest answer')]);
    expect(transcript.messages.map((message) => message.id)).toEqual([-1, -2]);
    expect(transcript.before).toBeUndefined();
    expect(transcript.hasOlder).toBe(true);
    for (let id = 3; id < 100; id++) transcript.appendLocal([message(-id)]);
    expect(transcript.messages).toHaveLength(20);
    transcript.dispose();
  });
});
