import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Annotation, ArticleView, Item } from '$lib/types';
const api = vi.hoisted(() => ({
  loadArticle: vi.fn(),
  cancelArticleRead: vi.fn(),
  listItemAnnotations: vi.fn(),
  createAnnotation: vi.fn(),
  updateAnnotation: vi.fn(),
  deleteAnnotation: vi.fn(),
  getFeedIcons: vi.fn(),
}));
vi.mock('$lib/api', () => api);
import PostReader from './PostReader.svelte';
const item: Item = {
  id: 1,
  feedId: 1,
  guidHash: 'one',
  title: 'First post',
  link: 'https://reader.example/one',
  preview: 'preview one',
  publishedAt: 1,
  fetchedAt: 1,
  isRead: false,
  readAt: null,
  isSaved: false,
};
const other = { ...item, id: 2, title: 'Second post', link: 'https://reader.example/two' };
function view(id: number, content = `article body ${id}`): ArticleView {
  return {
    itemId: id,
    originalUrl: item.link,
    finalUrl: item.link,
    title: null,
    contentHtml: `<p>${content}</p>`,
    rssContentHtml: null,
    fetchedAt: 1,
    cached: false,
    error: null,
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function props(current: Item | null = item) {
  return { item: current, feeds: [], onClose: vi.fn(), onToggleSaved: vi.fn() };
}
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  for (const fn of Object.values(api)) fn.mockReset();
  api.cancelArticleRead.mockResolvedValue(undefined);
  api.listItemAnnotations.mockResolvedValue([]);
  api.getFeedIcons.mockResolvedValue({});
});
describe('PostReader request ownership', () => {
  it('cancels an old selection and ignores its late body and annotation replies', async () => {
    const first = deferred<ArticleView>();
    const firstAnnotations = deferred<Annotation[]>();
    api.loadArticle.mockReturnValueOnce(first.promise).mockResolvedValueOnce(view(2));
    api.listItemAnnotations.mockReturnValueOnce(firstAnnotations.promise).mockResolvedValueOnce([]);
    const { rerender } = render(PostReader, { props: props() });
    await waitFor(() => expect(api.loadArticle).toHaveBeenCalledTimes(1));
    const revision = api.loadArticle.mock.calls[0][2];
    await rerender(props(other));
    await waitFor(() => expect(screen.getByText('article body 2')).toBeTruthy());
    expect(api.cancelArticleRead).toHaveBeenCalledWith(revision);
    first.resolve(view(1));
    firstAnnotations.reject(new Error('late annotations from first'));
    await settle();
    expect(screen.queryByText('article body 1')).toBeNull();
    expect(screen.queryByText(/late annotations from first/)).toBeNull();
    expect(screen.getByText('article body 2')).toBeTruthy();
  });
  it('keeps the new body and refresh control when an old manual refresh finishes', async () => {
    const refresh = deferred<ArticleView>();
    api.loadArticle
      .mockResolvedValueOnce(view(1))
      .mockReturnValueOnce(refresh.promise)
      .mockResolvedValueOnce(view(2));
    const { rerender } = render(PostReader, { props: props() });
    await waitFor(() => expect(screen.getByText('article body 1')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh full text' }));
    const revision = api.loadArticle.mock.calls[1][2];
    expect(api.loadArticle.mock.calls[1].slice(0, 2)).toEqual([1, true]);
    await rerender(props(other));
    await waitFor(() => expect(screen.getByText('article body 2')).toBeTruthy());
    expect(api.cancelArticleRead).toHaveBeenCalledWith(revision);
    refresh.resolve(view(1, 'old refreshed body'));
    await settle();
    expect(screen.queryByText('old refreshed body')).toBeNull();
    expect(screen.getByRole('button', { name: 'Refresh full text' }).hasAttribute('disabled')).toBe(
      false,
    );
  });
  it('does not refetch the body or annotations for read/save metadata changes', async () => {
    api.loadArticle.mockResolvedValue(view(1));
    const { rerender } = render(PostReader, { props: props() });
    await waitFor(() => expect(screen.getByText('article body 1')).toBeTruthy());
    await rerender(props({ ...item, isRead: true, isSaved: true }));
    await settle();
    expect(api.loadArticle).toHaveBeenCalledTimes(1);
    expect(api.listItemAnnotations).toHaveBeenCalledTimes(1);
    expect(api.cancelArticleRead).not.toHaveBeenCalled();
  });
  it('keeps successfully loaded HTML when annotations fail and names the failed action in the error', async () => {
    api.loadArticle.mockResolvedValue(view(1));
    api.listItemAnnotations.mockRejectedValue(new Error('annotations unavailable'));
    render(PostReader, { props: props() });
    await waitFor(() => expect(screen.getByText('article body 1')).toBeTruthy());
    const alert = screen.getByRole('alert');
    expect(alert.textContent).toContain('annotations unavailable');
    expect(alert.textContent).toContain('load');
  });

  it('lists a highlight with no note in the annotations index, with a header dot', async () => {
    api.loadArticle.mockResolvedValue(view(1));
    api.listItemAnnotations.mockResolvedValue([
      {
        id: 7,
        itemId: 1,
        kind: 'highlight',
        contentHash: 'h',
        startOffset: 0,
        endOffset: 5,
        quotedText: 'article',
        color: 'green',
        note: null,
        createdAt: 1,
        updatedAt: 1,
      },
    ]);
    render(PostReader, { props: props() });
    await waitFor(() => expect(screen.getByText('article body 1')).toBeTruthy());

    expect(screen.getByText('Highlights & notes')).toBeTruthy();
    expect(screen.getByText('“article”')).toBeTruthy();
    const jump = screen.getByRole('button', { name: 'Highlights & notes' });
    expect(jump.hasAttribute('disabled')).toBe(false);
  });
  it('cancels the current request on close without applying a late failure to another instance', async () => {
    const pending = deferred<ArticleView>();
    api.loadArticle.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(view(2));
    const { unmount } = render(PostReader, { props: props() });
    await waitFor(() => expect(api.loadArticle).toHaveBeenCalled());
    const revision = api.loadArticle.mock.calls[0][2];
    unmount();
    expect(api.cancelArticleRead).toHaveBeenCalledWith(revision);
    render(PostReader, { props: props(other) });
    await waitFor(() => expect(screen.getByText('article body 2')).toBeTruthy());
    expect(api.loadArticle.mock.calls[1][2]).toBeGreaterThan(revision);
    pending.reject(new Error('late failure'));
    await settle();
    expect(screen.queryByText(/late failure/)).toBeNull();
  });
  it('does not attach a completed annotation mutation to a newly selected post', async () => {
    const saved = deferred<Annotation>();
    api.createAnnotation.mockReturnValue(saved.promise);
    api.loadArticle.mockImplementation(async (id: number) => view(id));
    const { container, rerender } = render(PostReader, { props: props() });
    await waitFor(() => expect(screen.getByText('article body 1')).toBeTruthy());
    const root = container.querySelector('.content') as HTMLElement;
    const text = document.createTreeWalker(root, NodeFilter.SHOW_TEXT).nextNode()!;
    const range = document.createRange();
    range.setStart(text, 0);
    range.setEnd(text, 7);
    range.getBoundingClientRect = () => new DOMRect();
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    await fireEvent.mouseUp(root);
    await fireEvent.click(screen.getByRole('button', { name: 'Highlight yellow' }));
    await waitFor(() => expect(api.createAnnotation).toHaveBeenCalled());
    const input = api.createAnnotation.mock.calls[0][0];
    await rerender(props(other));
    await waitFor(() => expect(screen.getByText('article body 2')).toBeTruthy());
    saved.resolve({ id: 999, ...input, color: 'yellow', note: null, createdAt: 1, updatedAt: 1 });
    await settle();
    await settle();
    expect(container.querySelector('[data-annotation-id="999"]')).toBeNull();
  });
  it('restores the reader after HTML arrives, releases observers and respects a new user scroll', async () => {
    const callbacks = new Set<() => void>();
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(private callback: () => void) {}
        observe() {
          callbacks.add(this.callback);
        }
        disconnect() {
          callbacks.delete(this.callback);
        }
      },
    );
    try {
      const pending = deferred<ArticleView>();
      api.loadArticle.mockReturnValueOnce(pending.promise);
      const position = vi.fn();
      const { container, unmount } = render(PostReader, {
        props: { ...props(), initialScrollTop: 900, onScrollPositionChange: position },
      });
      const scroll = container.querySelector('.reader-scroll') as HTMLElement;
      expect(scroll.scrollTop).toBe(0);
      pending.resolve(view(1));
      await waitFor(() => expect(scroll.scrollTop).toBe(900));
      expect(position).toHaveBeenCalledWith(900);
      await fireEvent.wheel(scroll);
      scroll.scrollTop = 300;
      await fireEvent.scroll(scroll);
      for (const callback of [...callbacks]) callback();
      await settle();
      expect(scroll.scrollTop).toBe(300);
      expect(position).toHaveBeenLastCalledWith(300);
      unmount();
      expect(callbacks.size).toBe(0);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('opens the overflow menu, runs an action through it, and closes on Escape with focus restored', async () => {
    api.loadArticle.mockResolvedValue(view(1));
    api.listItemAnnotations.mockResolvedValue([]);
    const onToggleSaved = vi.fn();
    render(PostReader, { props: { ...props(), onToggleSaved } });
    await settle();

    const trigger = screen.getByRole('button', { name: 'More actions' });
    await fireEvent.click(trigger);
    const menu = screen.getByRole('menu', { name: 'More actions' });
    await fireEvent.click(within(menu).getByRole('menuitem', { name: /Save post/ }));
    expect(onToggleSaved).toHaveBeenCalledWith(1);
    expect(screen.queryByRole('menu')).toBeNull();

    await fireEvent.click(trigger);
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
});
