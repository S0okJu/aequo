import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import * as annotationModule from '$lib/annotation';
import type { Annotation } from '$lib/types';
import FeedContent from './FeedContent.svelte';

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

// A couple of real event-loop ticks (not just queued microtasks) - the
// overlay effect chains a `crypto.subtle.digest()` call, which resolves on
// its own macrotask, so a single same-tick flush is occasionally too early
// under load (e.g. the full suite running many files at once).
async function flushMicrotasks() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

function selectFirstTextNode(root: HTMLElement, start: number, end: number): Range {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const textNode = walker.nextNode() as Text;
  const range = document.createRange();
  range.setStart(textNode, start);
  range.setEnd(textNode, end);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  return range;
}

// jsdom has no layout engine - Range doesn't implement getBoundingClientRect
// at all (unlike a real browser, where it always exists). Stub it so
// selection-driven tests can run without reaching into FeedContent's code to
// special-case a jsdom-only gap.
if (!Range.prototype.getBoundingClientRect) {
  Range.prototype.getBoundingClientRect = function (): DOMRect {
    return {
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      width: 0,
      height: 0,
      x: 0,
      y: 0,
      toJSON() {},
    } as DOMRect;
  };
}

describe('FeedContent', () => {
  it('keeps allowed formatting while removing scripts, event handlers, and unsafe URLs', () => {
    const { container } = render(FeedContent, {
      props: {
        content:
          '<script>window.__feedXss = true</script><p onclick="alert(1)">Safe <strong>text</strong></p><img src="javascript:alert(1)" onerror="alert(1)"><a href="javascript:alert(1)">bad link</a>',
        baseUrl: 'https://feed.example/',
      },
    });

    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('p')?.hasAttribute('onclick')).toBe(false);
    expect(container.querySelector('strong')?.textContent).toBe('text');
    expect(container.querySelector('a')?.hasAttribute('href')).toBe(false);
    expect(container.textContent).toContain('bad link');
  });

  it('does not introduce stray whitespace around the injected HTML (would silently shift every annotation offset)', () => {
    const { container } = render(FeedContent, {
      props: { content: '<p>hello world</p>', baseUrl: 'https://feed.example/' },
    });
    const contentDiv = container.querySelector('.content') as HTMLElement;
    expect(annotationModule.extractPlainText(contentDiv)).toBe('hello world');
  });

  it('renders a highlight overlay from the annotations prop', async () => {
    const { container } = render(FeedContent, {
      props: {
        content: '<p>hello world</p>',
        baseUrl: 'https://feed.example/',
        annotations: [
          sampleAnnotation({ contentHash: await annotationModule.hashPlainText('hello world') }),
        ],
      },
    });

    await flushMicrotasks();

    const mark = container.querySelector('mark[data-annotation-id="1"]');
    expect(mark).not.toBeNull();
    expect(mark?.textContent).toBe('hello');
    expect(mark?.className).toContain('aequo-hl-yellow');
  });

  it('calls onReanchored when the stored hash is stale but the quote is still found', async () => {
    const onReanchored = vi.fn();
    render(FeedContent, {
      props: {
        content: '<p>prefix. hello world</p>',
        baseUrl: 'https://feed.example/',
        annotations: [
          sampleAnnotation({ contentHash: 'definitely-stale', startOffset: 0, endOffset: 5 }),
        ],
        onReanchored,
      },
    });

    // Re-anchoring hashes the text asynchronously (crypto.subtle), which can
    // take longer than a couple of macrotask turns under a full-suite load.
    await vi.waitFor(() => expect(onReanchored).toHaveBeenCalled());

    expect(onReanchored).toHaveBeenCalledTimes(1);
    const updates = onReanchored.mock.calls[0][0];
    expect(updates).toHaveLength(1);
    expect(updates[0].id).toBe(1);
  });

  it('reports orphaned annotations that cannot be relocated', async () => {
    const onOrphanedChange = vi.fn();
    render(FeedContent, {
      props: {
        content: '<p>completely different text</p>',
        baseUrl: 'https://feed.example/',
        annotations: [
          sampleAnnotation({
            contentHash: 'stale',
            quotedText: 'this text is nowhere to be found at all',
          }),
        ],
        onOrphanedChange,
      },
    });

    await flushMicrotasks();

    expect(onOrphanedChange).toHaveBeenCalledWith([1]);
  });

  it('truncates a long selection to 2000 chars before calling onAskAi', async () => {
    const onAskAi = vi.fn();
    const longText = 'a'.repeat(3000);
    const { container } = render(FeedContent, {
      props: {
        content: `<p>${longText}</p>`,
        baseUrl: 'https://feed.example/',
        onAskAi,
      },
    });
    await flushMicrotasks();

    const contentDiv = container.querySelector('.content') as HTMLElement;
    selectFirstTextNode(contentDiv, 0, 3000);
    contentDiv.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
    await flushMicrotasks();

    const toolbar = container.querySelector(
      '[aria-label="Ask AI about selection"]',
    ) as HTMLButtonElement;
    expect(toolbar).not.toBeNull();
    toolbar.click();

    expect(onAskAi).toHaveBeenCalledTimes(1);
    expect(onAskAi.mock.calls[0][0]).toHaveLength(2000);
  });

  it('only recomputes selection offsets on mouseup/keyup, not on every selectionchange tick', async () => {
    const offsetsSpy = vi.spyOn(annotationModule, 'selectionToOffsets');
    const { container } = render(FeedContent, {
      props: {
        content: '<p>hello world</p>',
        baseUrl: 'https://feed.example/',
      },
    });
    await flushMicrotasks();
    offsetsSpy.mockClear();

    const contentDiv = container.querySelector('.content') as HTMLElement;
    selectFirstTextNode(contentDiv, 0, 5);

    // Many selectionchange ticks (as a user drags across text fires
    // repeatedly) must not each trigger the expensive TreeWalker-based
    // offset computation - only the final mouseup/keyup does.
    for (let i = 0; i < 10; i++) {
      document.dispatchEvent(new Event('selectionchange'));
    }
    expect(offsetsSpy).not.toHaveBeenCalled();

    contentDiv.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
    expect(offsetsSpy).toHaveBeenCalledTimes(1);

    offsetsSpy.mockRestore();
  });

  it('still creates the memo after focusing the note textarea collapses window selection', async () => {
    // Regression test: focusing the popover's textarea moves focus off the
    // article text, which collapses `window.getSelection()` and fires a
    // `selectionchange` event just like clicking away would. That must not
    // be treated as "the user abandoned their selection" while the memo
    // popover is open, or Save silently does nothing (see handleSelectionChange).
    const onCreateAnnotation = vi.fn();
    const { container, getByPlaceholderText, getByRole } = render(FeedContent, {
      props: {
        content: '<p>hello world</p>',
        baseUrl: 'https://feed.example/',
        onCreateAnnotation,
      },
    });
    await flushMicrotasks();

    const contentDiv = container.querySelector('.content') as HTMLElement;
    selectFirstTextNode(contentDiv, 0, 5);
    contentDiv.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }));
    await flushMicrotasks();

    const memoButton = getByRole('button', { name: 'Add memo' }) as HTMLButtonElement;
    memoButton.click();
    await flushMicrotasks();

    // Simulate focusing the textarea: the window selection collapses and a
    // selectionchange ticks, just as it would in a real browser.
    window.getSelection()?.collapseToStart();
    document.dispatchEvent(new Event('selectionchange'));
    await flushMicrotasks();

    const textarea = getByPlaceholderText('Add a note…');
    await fireEvent.input(textarea, { target: { value: 'remember this' } });

    const saveButton = getByRole('button', { name: 'Save' }) as HTMLButtonElement;
    saveButton.click();
    await flushMicrotasks();

    expect(onCreateAnnotation).toHaveBeenCalledTimes(1);
    expect(onCreateAnnotation.mock.calls[0][0]).toMatchObject({
      kind: 'memo',
      note: 'remember this',
      quotedText: 'hello',
    });
  });
});
