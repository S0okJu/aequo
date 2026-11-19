import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ErrorCard from './ErrorCard.svelte';

describe('ErrorCard', () => {
  it('renders the title and body as an alert', () => {
    render(ErrorCard, { props: { title: "Couldn't save this", body: 'Network error' } });
    const card = screen.getByRole('alert');
    expect(card.textContent).toContain("Couldn't save this");
    expect(card.textContent).toContain('Network error');
  });

  it('omits the retry button when onRetry is not given', () => {
    render(ErrorCard, { props: { title: 'x', body: 'y' } });
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  });

  it('calls onRetry and disables itself while retrying', async () => {
    const onRetry = vi.fn();
    const { rerender } = render(ErrorCard, { props: { title: 'x', body: 'y', onRetry } });
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onRetry).toHaveBeenCalledTimes(1);

    await rerender({ title: 'x', body: 'y', onRetry, retrying: true });
    const retrying = screen.getByRole('button', { name: 'Retrying…' });
    expect(retrying.hasAttribute('disabled')).toBe(true);
  });

  it('omits the dismiss button when onDismiss is not given (AddFeedForm-style body-only card)', () => {
    render(ErrorCard, { props: { title: 'x', body: 'y' } });
    expect(screen.queryByLabelText('Dismiss error')).toBeNull();
  });

  it('calls onDismiss when the dismiss button is clicked', async () => {
    const onDismiss = vi.fn();
    render(ErrorCard, { props: { title: 'x', body: 'y', onDismiss } });
    await fireEvent.click(screen.getByLabelText('Dismiss error'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
