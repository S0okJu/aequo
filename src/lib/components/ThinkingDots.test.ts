import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ThinkingDots from './ThinkingDots.svelte';

describe('ThinkingDots', () => {
  it('renders the given label alongside the three dots', () => {
    const { container } = render(ThinkingDots, { props: { label: 'Thinking…' } });
    expect(screen.getByText('Thinking…')).toBeTruthy();
    expect(container.querySelectorAll('.thinking span')).toHaveLength(3);
  });
});
