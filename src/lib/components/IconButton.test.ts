import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import IconButton from './IconButton.svelte';

describe('IconButton', () => {
  it('uses the label as both the accessible name and the title by default', () => {
    render(IconButton, { props: { icon: 'close', label: 'Close' } });
    const button = screen.getByRole('button', { name: 'Close' });
    expect(button.getAttribute('title')).toBe('Close');
  });

  it('lets a separate title override the accessible name', () => {
    render(IconButton, {
      props: { icon: 'retry', label: 'Refresh full text', titleOverride: 'Cached copy' },
    });
    const button = screen.getByRole('button', { name: 'Refresh full text' });
    expect(button.getAttribute('title')).toBe('Cached copy');
  });

  it('reports aria-pressed as true/false for a toggle button either way', () => {
    render(IconButton, { props: { icon: 'star', label: 'Save', active: true } });
    expect(screen.getByRole('button', { name: 'Save' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('still reports aria-pressed="false" for a toggle that is currently off', () => {
    render(IconButton, { props: { icon: 'star-outline', label: 'Save', active: false } });
    expect(screen.getByRole('button', { name: 'Save' }).getAttribute('aria-pressed')).toBe(
      'false',
    );
  });

  it('omits aria-pressed for a plain action button', () => {
    render(IconButton, { props: { icon: 'close', label: 'Close' } });
    expect(screen.getByRole('button', { name: 'Close' }).hasAttribute('aria-pressed')).toBe(false);
  });

  it('renders an external link instead of a button when href is given', () => {
    render(IconButton, {
      props: { icon: 'external-link', label: 'Open original', href: 'https://example.com' },
    });
    const link = screen.getByRole('link', { name: 'Open original' });
    expect(link.getAttribute('target')).toBe('_blank');
    expect(link.getAttribute('rel')).toBe('noopener noreferrer');
  });

  it('calls onclick when clicked', async () => {
    const onclick = vi.fn();
    render(IconButton, { props: { icon: 'close', label: 'Close', onclick } });
    screen.getByRole('button', { name: 'Close' }).click();
    expect(onclick).toHaveBeenCalledTimes(1);
  });

  it('disables the button and leaves no onclick path when disabled', () => {
    render(IconButton, { props: { icon: 'close', label: 'Close', disabled: true } });
    expect(screen.getByRole('button', { name: 'Close' }).hasAttribute('disabled')).toBe(true);
  });
});
