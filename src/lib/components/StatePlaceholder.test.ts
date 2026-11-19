import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import StatePlaceholder from './StatePlaceholder.svelte';

describe('StatePlaceholder', () => {
  it('renders a busy status region for loading, with no own icon', () => {
    const { container } = render(StatePlaceholder, {
      props: { kind: 'loading', title: 'Loading the thing', body: 'One moment.' },
    });
    const region = screen.getByRole('status');
    expect(region.getAttribute('aria-busy')).toBe('true');
    expect(container.querySelector('.spinner')).not.toBeNull();
    expect(screen.getByText('Loading the thing')).toBeTruthy();
  });

  it('renders an empty state with no role and the given icon', () => {
    const { container } = render(StatePlaceholder, {
      props: { kind: 'empty', icon: 'inbox', title: 'Nothing here', body: 'Add a feed to start.' },
    });
    expect(container.querySelector('[role="status"]')).toBeNull();
    expect(container.querySelector('[role="alert"]')).toBeNull();
    expect(screen.getByText('Nothing here')).toBeTruthy();
  });

  it('renders an error state as an alert with the error icon, plus an action', () => {
    render(StatePlaceholder, {
      props: {
        kind: 'error',
        title: "Couldn't load this",
        body: 'Network error',
      },
    });
    expect(screen.getByRole('alert').textContent).toContain("Couldn't load this");
  });
});
