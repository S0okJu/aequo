import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ResourceSettings, ResourceStatus } from '$lib/types';

const api = vi.hoisted(() => ({
  getResourceSettings: vi.fn(),
  setResourceSettings: vi.fn(),
  setResourceVisibility: vi.fn(),
}));
vi.mock('$lib/api', () => api);
import SettingsPerformance from './SettingsPerformance.svelte';
import { resourcePreset, resourceState } from '$lib/resources.svelte';

function status(settings = resourcePreset('balanced'), revision = 0): ResourceStatus {
  return { snapshot: { settings, revision }, runningWork: 0, recoveryError: null };
}

describe('Performance settings', () => {
  beforeEach(() => {
    resourceState.status = null;
    resourceState.loading = true;
    resourceState.error = null;
    vi.resetAllMocks();
    api.getResourceSettings.mockResolvedValue(status());
    api.setResourceSettings.mockImplementation(
      async (revision: number, settings: ResourceSettings) => status(settings, revision + 1),
    );
  });

  it('applies a preset and persists individual edits as custom settings', async () => {
    render(SettingsPerformance);
    const light = await screen.findByRole('radio', { name: 'Light' });
    await fireEvent.click(light);
    await waitFor(() =>
      expect(api.setResourceSettings).toHaveBeenCalledWith(0, resourcePreset('light')),
    );
    await waitFor(() => expect(screen.getByText('Saved and applied.')).toBeTruthy());
    const articles = screen.getByLabelText('Concurrent article extraction') as HTMLSelectElement;
    expect(articles.disabled).toBe(false);
    expect((screen.getByLabelText('Fetch full text in advance') as HTMLInputElement).checked).toBe(
      false,
    );
    await fireEvent.change(screen.getByLabelText('App-managed cache budget'), {
      target: { value: '64' },
    });
    await waitFor(() =>
      expect(api.setResourceSettings).toHaveBeenLastCalledWith(1, {
        ...resourcePreset('light'),
        mode: 'custom',
        cacheBudgetMib: 64,
      }),
    );
  });

  it('serializes rapid edits and keeps the last intent with the new revision', async () => {
    let finish: (value: ResourceStatus) => void = () => {};
    api.setResourceSettings.mockImplementationOnce(
      () =>
        new Promise<ResourceStatus>((resolve) => {
          finish = resolve;
        }),
    );
    render(SettingsPerformance);
    await screen.findByRole('radio', { name: 'Light' });
    await fireEvent.change(screen.getByLabelText('Concurrent feed updates'), {
      target: { value: '1' },
    });
    await waitFor(() => expect(api.setResourceSettings).toHaveBeenCalledTimes(1));
    await fireEvent.change(screen.getByLabelText('Keep list pages in memory'), {
      target: { value: '2' },
    });
    expect(api.setResourceSettings).toHaveBeenCalledTimes(1);
    const first = { ...resourcePreset('balanced'), mode: 'custom' as const, feedConcurrency: 1 };
    finish(status(first, 1));
    await waitFor(() =>
      expect(api.setResourceSettings).toHaveBeenLastCalledWith(1, { ...first, retainedPages: 2 }),
    );
    await waitFor(() => expect(screen.getByText('Saved and applied.')).toBeTruthy());
  });

  it('shows a retryable error instead of a blank screen when the initial load fails', async () => {
    api.getResourceSettings.mockRejectedValueOnce(new Error('disk unavailable'));
    render(SettingsPerformance);

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('disk unavailable');

    api.getResourceSettings.mockResolvedValueOnce(status());
    await fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    await screen.findByRole('radio', { name: 'Light' });
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('reloads applied values on save failure and permits explicit damaged-setting recovery', async () => {
    api.setResourceSettings.mockRejectedValueOnce(new Error('disk unavailable'));
    const view = render(SettingsPerformance);
    await fireEvent.click(await screen.findByRole('radio', { name: 'Light' }));
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toContain('disk unavailable'),
    );
    expect((screen.getByLabelText('Concurrent feed updates') as HTMLSelectElement).value).toBe('2');
    view.unmount();
    resourceState.status = null;
    api.getResourceSettings.mockResolvedValue({ ...status(), recoveryError: 'damaged JSON' });
    render(SettingsPerformance);
    await screen.findByText(/Saved resource settings could not be read/);
    await fireEvent.click(screen.getByRole('button', { name: 'Restore defaults' }));
    await waitFor(() =>
      expect(api.setResourceSettings).toHaveBeenLastCalledWith(0, resourcePreset('balanced')),
    );
    await waitFor(() =>
      expect(screen.queryByText(/Saved resource settings could not be read/)).toBeNull(),
    );
  });
});
