import { render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import PendingTestHarness from './pendingTestHarness.svelte';

/**
 * `delayed()` uses `$effect`/`$state`, which only run inside a mounted
 * component - this drives it through a tiny harness component via
 * @testing-library/svelte rather than calling it from plain test code.
 */
describe('delayed', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('stays false until the busy duration passes', async () => {
    render(PendingTestHarness, { props: { busy: true, ms: 200 } });
    const value = screen.getByTestId('value');
    expect(value.textContent).toBe('false');

    await vi.advanceTimersByTimeAsync(199);
    expect(value.textContent).toBe('false');

    await vi.advanceTimersByTimeAsync(1);
    expect(value.textContent).toBe('true');
  });

  it('snaps back to false as soon as busy goes false, even before the delay elapses', async () => {
    const { rerender } = render(PendingTestHarness, { props: { busy: true, ms: 200 } });
    const value = screen.getByTestId('value');

    await vi.advanceTimersByTimeAsync(100);
    expect(value.textContent).toBe('false');

    await rerender({ busy: false, ms: 200 });
    expect(value.textContent).toBe('false');

    await vi.advanceTimersByTimeAsync(1000);
    expect(value.textContent).toBe('false');
  });

  it('never flips true for work that finishes before the delay', async () => {
    const { rerender } = render(PendingTestHarness, { props: { busy: true, ms: 200 } });

    await vi.advanceTimersByTimeAsync(50);
    await rerender({ busy: false, ms: 200 });
    await vi.advanceTimersByTimeAsync(500);

    expect(screen.getByTestId('value').textContent).toBe('false');
  });
});
