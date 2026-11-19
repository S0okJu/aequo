import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AiProviderStatus } from '$lib/types';

const api = vi.hoisted(() => ({
  checkAiProviders: vi.fn(),
}));
vi.mock('$lib/api', () => api);
import SettingsAi from './SettingsAi.svelte';

const installed: AiProviderStatus[] = [
  { provider: 'codex', installed: true, version: '1.2.3', error: null },
  { provider: 'claude', installed: true, version: '4.5.6', error: null },
];

describe('SettingsAi', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    localStorage.clear();
  });

  it('makes the default AI selectable with the same named choices as the chat panel', async () => {
    api.checkAiProviders.mockResolvedValue(installed);
    render(SettingsAi);
    const picker = screen.getByRole('combobox', { name: 'AI for new conversations' });
    await fireEvent.change(picker, { target: { value: 'claude' } });
    expect(picker).toHaveProperty('value', 'claude');
    expect(localStorage.getItem('aequo:aiProvider')).toBe('claude');
    expect(screen.getByText('Saved')).toBeTruthy();
  });

  it('shows an installed state for each provider on a successful check', async () => {
    api.checkAiProviders.mockResolvedValue(installed);
    render(SettingsAi);
    await waitFor(() => expect(screen.getAllByText('Installed')).toHaveLength(2));
  });

  it('surfaces a retryable error instead of spinning forever when the check fails', async () => {
    api.checkAiProviders.mockRejectedValueOnce(new Error('cli not found'));
    render(SettingsAi);

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain("Couldn't check CLI status");
    expect(alert.textContent).toContain('cli not found');

    // Regression guard for the bug this fixes: previously a rejection left
    // `checking` cleared but `statuses` null forever, so every row stayed
    // on the `checking || !status` branch - a permanent spinner, no error.
    expect(screen.queryAllByRole('status', { name: 'Loading' })).toHaveLength(0);

    api.checkAiProviders.mockResolvedValueOnce(installed);
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(screen.getAllByText('Installed')).toHaveLength(2));
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
