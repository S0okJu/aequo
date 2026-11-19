import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Item } from '$lib/types';
const api = vi.hoisted(() => ({ listItems: vi.fn() }));
vi.mock('$lib/api', () => api);
import AiArticleMentions from './AiArticleMentions.svelte';

const article = (id: number, title: string): Item => ({
  id,
  title,
  feedId: 1,
  guidHash: `${id}`,
  link: null,
  preview: null,
  publishedAt: 0,
  fetchedAt: 0,
  isRead: false,
  readAt: null,
  isSaved: false,
});
const props = () => ({
  id: 'articles',
  query: '',
  currentItem: null,
  selectedIds: [],
  onSelect: vi.fn(async () => true),
  onClose: vi.fn(),
  onActiveChange: vi.fn(),
});

beforeEach(() => api.listItems.mockReset());

describe('article mention search', () => {
  it('ignores a late result for a previous search', async () => {
    let resolveOld!: (items: Item[]) => void;
    api.listItems
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
      )
      .mockResolvedValueOnce([article(2, 'New result')]);
    const { rerender } = render(AiArticleMentions, { props: { ...props(), query: 'old' } });
    await waitFor(() => expect(api.listItems).toHaveBeenCalledWith({ query: 'old' }, 30));
    await rerender({ query: 'new' });
    expect(await screen.findByRole('option', { name: /New result/ })).toBeTruthy();
    resolveOld([article(1, 'Old result')]);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByRole('option', { name: /Old result/ })).toBeNull();
  });

  it('retries search failures and excludes already attached results', async () => {
    api.listItems
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce([article(1, 'Attached'), article(2, 'Another article')]);
    render(AiArticleMentions, { props: { ...props(), selectedIds: [1] } });
    await screen.findByText('Could not search articles.');
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await screen.findByRole('option', { name: /Another article/ })).toBeTruthy();
    expect(screen.queryByRole('option', { name: /Attached/ })).toBeNull();
  });

  it('does not search or select more than the conversation limit', () => {
    render(AiArticleMentions, { props: { ...props(), atLimit: true } });
    expect(screen.getByText(/up to 20 articles/)).toBeTruthy();
    expect(screen.queryByRole('option')).toBeNull();
    expect(api.listItems).not.toHaveBeenCalled();
  });
});
