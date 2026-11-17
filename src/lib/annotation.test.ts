import { describe, expect, it } from 'vitest';

import {
  applyOverlay,
  extractPlainText,
  hashPlainText,
  offsetsToRange,
  reconcileAnnotations,
  removeOverlay,
  selectionToOffsets,
} from './annotation';
import type { Annotation } from './types';

function sampleAnnotation(overrides: Partial<Annotation> = {}): Annotation {
  return {
    id: 1,
    itemId: 1,
    kind: 'highlight',
    contentHash: 'stale-hash',
    startOffset: 0,
    endOffset: 5,
    quotedText: 'hello',
    color: 'yellow',
    note: null,
    createdAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

function el(html: string): HTMLElement {
  const div = document.createElement('div');
  div.innerHTML = html;
  return div;
}

describe('extractPlainText', () => {
  it('concatenates text across nested elements', () => {
    const root = el('<p>hello <strong>world</strong></p>');
    expect(extractPlainText(root)).toBe('hello world');
  });
});

describe('offsetsToRange / selectionToOffsets', () => {
  it('round-trips an offset pair through a Range', () => {
    const root = el('<p>hello <strong>world</strong></p>');
    const range = offsetsToRange(root, 6, 11);
    expect(range?.toString()).toBe('world');

    const offsets = selectionToOffsets(root, range!);
    expect(offsets).toEqual({ start: 6, end: 11 });
  });

  it('returns null for an empty or inverted range', () => {
    const root = el('<p>hello</p>');
    expect(offsetsToRange(root, 3, 3)).toBeNull();
    expect(offsetsToRange(root, 5, 2)).toBeNull();
  });
});

describe('hashPlainText', () => {
  it('is deterministic for the same text', async () => {
    const a = await hashPlainText('hello world');
    const b = await hashPlainText('hello world');
    expect(a).toBe(b);
  });

  it('differs for different text', async () => {
    const a = await hashPlainText('hello world');
    const b = await hashPlainText('hello world!');
    expect(a).not.toBe(b);
  });
});

describe('applyOverlay / removeOverlay', () => {
  it('wraps a highlight range in a <mark> with the right metadata', () => {
    const root = el('<p>hello world</p>');
    applyOverlay(root, [
      sampleAnnotation({ startOffset: 6, endOffset: 11, kind: 'highlight', color: 'pink' }),
    ]);

    const mark = root.querySelector('mark');
    expect(mark).not.toBeNull();
    expect(mark?.textContent).toBe('world');
    expect(mark?.className).toBe('aequo-hl aequo-hl-pink');
    expect(mark?.getAttribute('data-annotation-id')).toBe('1');
    expect(extractPlainText(root)).toBe('hello world');
  });

  it('wraps a memo range in a <span> without inserting extra text', () => {
    const root = el('<p>hello world</p>');
    applyOverlay(root, [
      sampleAnnotation({ kind: 'memo', color: null, note: 'a note', startOffset: 0, endOffset: 5 }),
    ]);

    const span = root.querySelector('span[data-kind="memo"]');
    expect(span?.textContent).toBe('hello');
    expect(extractPlainText(root)).toBe('hello world');
  });

  it('is idempotent: reapplying does not nest marks or drift offsets', () => {
    const root = el('<p>hello world</p>');
    const annotations = [sampleAnnotation({ startOffset: 0, endOffset: 5 })];
    applyOverlay(root, annotations);
    applyOverlay(root, annotations);
    applyOverlay(root, annotations);

    expect(root.querySelectorAll('mark').length).toBe(1);
    expect(extractPlainText(root)).toBe('hello world');
  });

  it('removeOverlay restores plain text nodes and merges them back', () => {
    const root = el('<p>hello world</p>');
    applyOverlay(root, [sampleAnnotation({ startOffset: 0, endOffset: 5 })]);
    removeOverlay(root);

    expect(root.querySelector('mark')).toBeNull();
    expect(root.querySelector('p')?.childNodes.length).toBe(1);
    expect(extractPlainText(root)).toBe('hello world');
  });

  it('nests a highlight and a memo sharing the identical range without corrupting the text', () => {
    const root = el('<p>hello world</p>');
    applyOverlay(root, [
      sampleAnnotation({ id: 1, kind: 'highlight', color: 'yellow', startOffset: 0, endOffset: 5 }),
      sampleAnnotation({
        id: 2,
        kind: 'memo',
        color: null,
        note: 'note',
        startOffset: 0,
        endOffset: 5,
      }),
    ]);

    expect(extractPlainText(root)).toBe('hello world');
    expect(root.querySelectorAll('[data-aequo-overlay]').length).toBe(2);
  });
});

describe('reconcileAnnotations', () => {
  it('keeps an annotation unchanged when the content hash matches', async () => {
    const plainText = 'hello world';
    const hash = await hashPlainText(plainText);
    const annotation = sampleAnnotation({
      contentHash: hash,
      startOffset: 0,
      endOffset: 5,
      quotedText: 'hello',
    });

    const result = await reconcileAnnotations(plainText, [annotation]);
    expect(result.rendered).toEqual([annotation]);
    expect(result.toPersist).toEqual([]);
    expect(result.orphanedIds).toEqual([]);
  });

  it('relocates via exact match when the text shifted but the quote is still present', async () => {
    const plainText = 'prefix added. hello world';
    const annotation = sampleAnnotation({
      contentHash: 'stale',
      startOffset: 0,
      endOffset: 5,
      quotedText: 'hello',
    });

    const result = await reconcileAnnotations(plainText, [annotation]);
    expect(result.orphanedIds).toEqual([]);
    expect(result.rendered[0].startOffset).toBe(plainText.indexOf('hello'));
    expect(result.rendered[0].endOffset).toBe(plainText.indexOf('hello') + 'hello'.length);
    expect(result.toPersist).toHaveLength(1);
    expect(result.toPersist[0].id).toBe(annotation.id);
  });

  it('picks the match closest to the stored offset when the quote appears multiple times', async () => {
    const plainText = 'hello there, far away hello here, hello nearby';
    // Stored offset points near the third occurrence.
    const thirdOccurrence = plainText.lastIndexOf('hello');
    const annotation = sampleAnnotation({
      contentHash: 'stale',
      startOffset: thirdOccurrence - 1,
      endOffset: thirdOccurrence - 1 + 'hello'.length,
      quotedText: 'hello',
    });

    const result = await reconcileAnnotations(plainText, [annotation]);
    expect(result.rendered[0].startOffset).toBe(thirdOccurrence);
  });

  it('relocates via fuzzy match when the quote text changed slightly', async () => {
    const plainText = 'the quick brown fox jumps over the lazy dog';
    const annotation = sampleAnnotation({
      contentHash: 'stale',
      startOffset: 4,
      endOffset: 19,
      quotedText: 'the quack brown fo',
    });

    const result = await reconcileAnnotations(plainText, [annotation]);
    expect(result.orphanedIds).toEqual([]);
    expect(result.toPersist).toHaveLength(1);
  });

  it('orphans an annotation whose quoted text is gone entirely', async () => {
    const plainText = 'completely different content';
    const annotation = sampleAnnotation({
      contentHash: 'stale',
      quotedText: 'this text does not exist anywhere nearby at all',
    });

    const result = await reconcileAnnotations(plainText, [annotation]);
    expect(result.rendered).toEqual([]);
    expect(result.toPersist).toEqual([]);
    expect(result.orphanedIds).toEqual([annotation.id]);
  });
});

describe('performance budget (M15)', () => {
  it('reconciles 200+ annotations against a long document within a generous budget', async () => {
    const paragraph =
      'The quick brown fox jumps over the lazy dog. This sentence repeats many times to build a long document. ';
    const plainText = paragraph.repeat(300);

    const annotations: Annotation[] = [];
    for (let i = 0; i < 220; i++) {
      const start = (i * 17) % (plainText.length - 20);
      annotations.push(
        sampleAnnotation({
          id: i + 1,
          contentHash: 'stale',
          startOffset: start,
          endOffset: start + 10,
          quotedText: plainText.slice(start, start + 10),
        }),
      );
    }

    const startedAt = performance.now();
    const result = await reconcileAnnotations(plainText, annotations);
    const elapsed = performance.now() - startedAt;

    expect(result.rendered.length + result.orphanedIds.length).toBe(annotations.length);
    // Generous, CI-tolerant budget - this only needs to catch an
    // accidental O(n^2)-or-worse regression, not enforce a tight SLA.
    expect(elapsed).toBeLessThan(2000);

    const root = el(`<p>${plainText}</p>`);
    const overlayStartedAt = performance.now();
    applyOverlay(root, result.rendered);
    const overlayElapsed = performance.now() - overlayStartedAt;
    expect(overlayElapsed).toBeLessThan(2000);
    expect(extractPlainText(root)).toBe(plainText);
  });
});
