export interface ArticleMention {
  start: number;
  end: number;
  query: string;
}

/** Only the token at a collapsed caret is a mention; email addresses are plain text. */
export function articleMentionAt(text: string, start: number, end = start): ArticleMention | null {
  if (start !== end) return null;
  const match = text.slice(0, start).match(/(?:^|[\s(])@([^\s@]*)$/u);
  if (!match) return null;
  return { start: start - match[1].length - 1, end: start, query: match[1] };
}
