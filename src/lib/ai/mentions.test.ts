import { describe, expect, it } from 'vitest';
import { articleMentionAt } from './mentions';

describe('article mention tokens', () => {
  it('finds the token at the caret without confusing email addresses or selected text', () => {
    expect(articleMentionAt('Compare @this', 13)).toEqual({ start: 8, end: 13, query: 'this' });
    expect(articleMentionAt('(@보안)', 4)).toEqual({ start: 1, end: 4, query: '보안' });
    expect(articleMentionAt('mail me@host.test', 17)).toBeNull();
    expect(articleMentionAt('@this', 0, 5)).toBeNull();
    expect(articleMentionAt('@this ', 6)).toBeNull();
  });

  it('can resolve a token in the middle of a question and after a line break', () => {
    expect(articleMentionAt('Compare @post with another', 13)).toEqual({
      start: 8,
      end: 13,
      query: 'post',
    });
    expect(articleMentionAt('Question\n@', 10)).toEqual({ start: 9, end: 10, query: '' });
  });
});
