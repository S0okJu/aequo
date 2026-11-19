import { describe, expect, it } from 'vitest';
import { renderMarkdown } from './markdown';
describe('article citations', () => {
  it('links only selected sources and leaves unknown sources and code alone', () => {
    const html = renderMarkdown('Use [1] and [2]. Code `[1]`.\n\n```\n[1]\n```', new Map([[1, 7]]));
    const node = document.createElement('div');
    node.innerHTML = html;
    expect(node.querySelectorAll('a[data-article-id]')).toHaveLength(1);
    expect(node.querySelector('a')?.dataset.articleId).toBe('7');
    expect(node.textContent).toContain('[2]');
    expect(node.querySelectorAll('code a')).toHaveLength(0);
  });
  it('renders compact accessible citations and lists each referenced source once with escaped titles', () => {
    const node = document.createElement('div');
    const title = 'A "quoted" <img src=x onerror=alert(1)> article';
    node.innerHTML = renderMarkdown(
      'First [1]. Again [1]. Unknown [3]. Code `[2]`.',
      new Map([
        [1, { itemId: 7, title }],
        [2, { itemId: 8, title: 'Unused article' }],
      ]),
      { sourceLabel: '출처', includeSources: true },
    );
    const badges = node.querySelectorAll('a.citation');
    expect(badges).toHaveLength(2);
    expect(badges[0].textContent).toBe('1');
    expect(badges[0].getAttribute('aria-label')).toBe(`출처 1: ${title}`);
    expect(badges[0].getAttribute('title')).toBe(title);
    const sources = node.querySelectorAll('.answer-source');
    expect(sources).toHaveLength(1);
    expect(sources[0].getAttribute('data-article-id')).toBe('7');
    expect(sources[0].textContent).toContain(title);
    expect(node.querySelector('img')).toBeNull();
    expect(node.textContent).toContain('[3]');
    expect(node.querySelector('code a')).toBeNull();
  });

  it('escapes model HTML and does not nest citations in ordinary links', () => {
    const node = document.createElement('div');
    node.innerHTML = renderMarkdown(
      '<script>bad()</script> [[1]](https://example.com)',
      new Map([[1, 7]]),
    );
    expect(node.querySelector('script')).toBeNull();
    expect(node.querySelector('a a')).toBeNull();
  });

  it('re-highlights a sent @article reference by its exact label text, leaving the rest untouched', () => {
    const node = document.createElement('div');
    node.innerHTML = renderMarkdown('@A "quoted" post summarize this', new Map(), {
      mentionLabels: new Map([['@A "quoted" post', 9]]),
    });
    const mention = node.querySelector('a.mention-highlight');
    expect(mention?.getAttribute('data-article-id')).toBe('9');
    expect(mention?.textContent).toBe('@A "quoted" post');
    expect(node.textContent).toBe('@A "quoted" post summarize this');
    expect(node.querySelector('code a')).toBeNull();
  });
});

describe('block formatting', () => {
  it('renders a GFM table with alignment, leaving a pipe row with no separator as plain text', () => {
    const node = document.createElement('div');
    node.innerHTML = renderMarkdown(
      '| Name | Count |\n| :-- | --: |\n| Alpha | 1 |\n| Beta | 2 |\n\nNot a table: | a | b |',
    );
    const table = node.querySelector('table');
    expect(table).not.toBeNull();
    expect(node.querySelectorAll('thead th')).toHaveLength(2);
    expect(node.querySelectorAll('tbody tr')).toHaveLength(2);
    expect(node.querySelector('thead th')?.getAttribute('style')).toBe('text-align:left');
    expect(node.querySelectorAll('thead th')[1].getAttribute('style')).toBe('text-align:right');
    expect(node.querySelector('tbody td')?.textContent).toBe('Alpha');
    expect(node.textContent).toContain('Not a table: | a | b |');
  });

  it('renders a leading > quote as a blockquote, separate from the text that follows it', () => {
    const node = document.createElement('div');
    node.innerHTML = renderMarkdown('> quoted line one\n> quoted line two\n\nMy actual question');
    const quote = node.querySelector('blockquote');
    expect(quote?.textContent).toBe('quoted line onequoted line two');
    expect(quote?.querySelectorAll('p')).toHaveLength(2);
    expect(node.querySelector('p:not(blockquote p)')?.textContent).toBe('My actual question');
  });

  it('skips the fallback source list under a short status line but keeps it for a real citation', () => {
    const short = renderMarkdown('Checking the article now.', new Map(), {
      includeSources: true,
      fallbackSources: [{ itemId: 7, title: 'Some article' }],
    });
    expect(short).not.toContain('answer-sources');

    const withCitation = renderMarkdown('See [1].', new Map([[1, 7]]), {
      includeSources: true,
      fallbackSources: [{ itemId: 7, title: 'Some article' }],
    });
    expect(withCitation).toContain('answer-sources');
  });
});
