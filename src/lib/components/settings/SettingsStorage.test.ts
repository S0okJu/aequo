import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CleanupPreview, RetentionSettings } from '$lib/types';

const api = vi.hoisted(() => ({
  getRetentionSettings: vi.fn(),
  previewRetentionCleanup: vi.fn(),
  runRetentionCleanup: vi.fn(),
  setRetentionSettings: vi.fn(),
}));
vi.mock('$lib/api', () => api);
import SettingsStorage from './SettingsStorage.svelte';

const retention: RetentionSettings = { readRetentionDays: 90, fullTextRetentionDays: 30 };
const preview: CleanupPreview = {
  articlesToRemove: 3,
  fullTextsToClear: 2,
  estimatedBytesToFree: 2048,
};

describe('SettingsStorage', () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it('loads and shows the retention controls', async () => {
    api.getRetentionSettings.mockResolvedValue(retention);
    api.previewRetentionCleanup.mockResolvedValue(preview);
    render(SettingsStorage);
    await screen.findByText('Unread and saved posts are always kept.');
  });

  it('prevents cleanup and policy edits until saving and preview refresh finish', async () => {
    api.getRetentionSettings.mockResolvedValue(retention);
    api.previewRetentionCleanup.mockResolvedValue(preview);
    let finishSave!: () => void;
    api.setRetentionSettings.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finishSave = resolve;
        }),
    );
    render(SettingsStorage);
    const option = within(
      await screen.findByRole('radiogroup', { name: 'Keep read posts' }),
    ).getByRole('radio', { name: '180 days' });
    await fireEvent.click(option);
    expect(screen.getByText('Saving…')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Clean up now' })).toHaveProperty('disabled', true);
    expect(option).toHaveProperty('disabled', true);
    finishSave();
    await waitFor(() => expect(option).toHaveProperty('disabled', false));
    expect(screen.getByRole('button', { name: 'Clean up now' })).toHaveProperty('disabled', false);
  });

  it('keeps the saved policy and disables cleanup when the new preview fails', async () => {
    api.getRetentionSettings.mockResolvedValue(retention);
    api.previewRetentionCleanup
      .mockResolvedValueOnce(preview)
      .mockRejectedValueOnce(new Error('preview unavailable'));
    api.setRetentionSettings.mockResolvedValue(undefined);
    render(SettingsStorage);
    const option = within(
      await screen.findByRole('radiogroup', { name: 'Keep read posts' }),
    ).getByRole('radio', { name: '180 days' });
    await fireEvent.click(option);
    await screen.findByText(
      'Policy saved. Could not refresh the cleanup preview: Error: preview unavailable',
    );
    expect(option.getAttribute('aria-checked')).toBe('true');
    expect(screen.getByRole('button', { name: 'Clean up now' })).toHaveProperty('disabled', true);
  });

  it('shows a retryable error instead of a blank screen when the load fails', async () => {
    api.getRetentionSettings.mockRejectedValueOnce(new Error('disk error'));
    api.previewRetentionCleanup.mockRejectedValueOnce(new Error('disk error'));
    render(SettingsStorage);

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain("Couldn't load this");
    expect(alert.textContent).toContain('disk error');

    api.getRetentionSettings.mockResolvedValueOnce(retention);
    api.previewRetentionCleanup.mockResolvedValueOnce(preview);
    await fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
    expect(screen.getByText('Unread and saved posts are always kept.')).toBeTruthy();
  });
});
