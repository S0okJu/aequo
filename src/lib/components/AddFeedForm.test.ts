import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { addFeed, updateFeedOrganization } = vi.hoisted(() => ({
  addFeed: vi.fn(),
  updateFeedOrganization: vi.fn(),
}));
vi.mock('$lib/api', () => ({ addFeed, updateFeedOrganization }));

import AddFeedForm from './AddFeedForm.svelte';

const folders = [
  { id: 3, name: 'Tech', sortOrder: 0, createdAt: 0 },
  { id: 4, name: 'News', sortOrder: 1, createdAt: 0 },
];

describe('AddFeedForm (dialog layout)', () => {
  beforeEach(() => {
    addFeed.mockReset().mockResolvedValue(42);
    updateFeedOrganization.mockReset().mockResolvedValue(true);
  });

  it('adds the feed, files it into the chosen folder and reports its id', async () => {
    const onAdded = vi.fn();
    render(AddFeedForm, { props: { folders, defaultFolderId: 3, onAdded, onCancel: vi.fn() } });

    await fireEvent.input(screen.getByLabelText('Feed URL'), {
      target: { value: 'https://blog.example/feed.xml' },
    });
    await fireEvent.change(screen.getByLabelText('Folder'), { target: { value: '4' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add feed' }));

    await vi.waitFor(() => expect(onAdded).toHaveBeenCalledWith(42));
    expect(addFeed).toHaveBeenCalledWith('https://blog.example/feed.xml');
    expect(updateFeedOrganization).toHaveBeenCalledWith(42, 4, false);
  });

  it('leaves the feed unfiled when no folder is chosen, and cancels without adding', async () => {
    const onAdded = vi.fn();
    const onCancel = vi.fn();
    render(AddFeedForm, { props: { folders, onAdded, onCancel } });

    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledOnce();

    await fireEvent.input(screen.getByLabelText('Feed URL'), {
      target: { value: 'https://blog.example/feed.xml' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Add feed' }));
    await vi.waitFor(() => expect(onAdded).toHaveBeenCalledWith(42));
    expect(updateFeedOrganization).not.toHaveBeenCalled();
  });

  it('shows an invalid URL as a field message next to the input, not a card, and never calls addFeed', async () => {
    render(AddFeedForm, { props: { folders, onAdded: vi.fn(), onCancel: vi.fn() } });
    const input = screen.getByLabelText('Feed URL');
    await fireEvent.input(input, { target: { value: 'not a url' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add feed' }));

    const message = await screen.findByRole('alert');
    expect(message.textContent).toContain("doesn't look like a valid URL");
    expect(message.classList.contains('field-error')).toBe(true);
    expect(document.activeElement).toBe(input);
    expect(addFeed).not.toHaveBeenCalled();
  });

  it('offers a retry card for a failed add, which resubmits the same URL', async () => {
    addFeed.mockRejectedValueOnce(new Error('request timed out'));
    render(AddFeedForm, { props: { folders, onAdded: vi.fn(), onCancel: vi.fn() } });
    await fireEvent.input(screen.getByLabelText('Feed URL'), {
      target: { value: 'https://blog.example/feed.xml' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Add feed' }));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('timed out');

    addFeed.mockResolvedValueOnce(99);
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await vi.waitFor(() => expect(addFeed).toHaveBeenCalledTimes(2));
    expect(addFeed).toHaveBeenLastCalledWith('https://blog.example/feed.xml');
  });
});
