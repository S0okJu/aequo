import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { UpdateSnapshot } from '$lib/updates.svelte';
const api = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: api.invoke, isTauri: () => true }));
import { acceptUpdateSnapshot, updateState } from '$lib/updates.svelte';
import SettingsUpdates from './SettingsUpdates.svelte';

function snapshot(overrides: Partial<UpdateSnapshot> = {}): UpdateSnapshot {
  return {
    revision: 1,
    operationId: 1,
    currentVersion: '1.0.1',
    phase: 'idle',
    autoUpdate: true,
    enabled: true,
    lastSuccessfulCheckAt: null,
    release: null,
    downloadedBytes: 0,
    totalBytes: null,
    error: null,
    appliedVersion: null,
    pendingInstalledVersion: null,
    ...overrides,
  };
}
const release = {
  version: '1.0.2',
  notes: '<script>untrusted()</script>',
  publishedAt: '2026-11-19T00:00:00Z',
  releaseUrl: 'https://github.com/S0okJu/aequo/releases/tag/v1.0.2',
};

describe('update settings', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    updateState.snapshot = null;
    updateState.error = null;
    updateState.pending = false;
    api.invoke.mockResolvedValue(snapshot());
  });
  it('shows the running version and persists an explicit OFF choice', async () => {
    render(SettingsUpdates);
    await screen.findByText('1.0.1');
    const toggle = screen.getByRole('switch', { name: 'Automatic updates' });
    await fireEvent.click(toggle);
    expect(api.invoke).toHaveBeenCalledWith('set_update_settings', { autoUpdate: false });
  });
  it('provides a manual install and renders release notes as plain text', async () => {
    api.invoke.mockResolvedValue(snapshot({ phase: 'available', autoUpdate: false, release }));
    const { container } = render(SettingsUpdates);
    await screen.findByText(release.notes);
    expect(container.querySelector('script')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Install update' }));
    expect(api.invoke).toHaveBeenCalledWith('install_available_update', undefined);
  });
  it('shows unknown-length progress without an invented percentage', async () => {
    api.invoke.mockResolvedValue(snapshot({ phase: 'downloading', downloadedBytes: 1024 * 1024 }));
    render(SettingsUpdates);
    const progress = await screen.findByRole('progressbar');
    expect(progress.hasAttribute('value')).toBe(false);
    expect(screen.getByRole('button', { name: 'Check now' })).toHaveProperty('disabled', true);
    acceptUpdateSnapshot(
      snapshot({ revision: 2, phase: 'downloading', downloadedBytes: 1024, totalBytes: 2048 }),
    );
    await waitFor(() => expect(progress.getAttribute('value')).toBe('50'));
  });
  it('keeps confirmation failures separate from a latest-version result', async () => {
    api.invoke.mockResolvedValue(
      snapshot({ phase: 'error', error: { stage: 'check', code: 'http', retryable: true } }),
    );
    render(SettingsUpdates);
    expect((await screen.findByRole('alert')).textContent).toContain('Could not reach');
    expect(screen.queryByText('You are up to date')).toBeNull();
  });
  it('offers restart and later after installation while keeping the current version', async () => {
    api.invoke.mockResolvedValue(
      snapshot({ phase: 'restart_required', pendingInstalledVersion: '1.0.2' }),
    );
    render(SettingsUpdates);
    await screen.findByText('1.0.1');
    await fireEvent.click(screen.getByRole('button', { name: 'Later' }));
    expect(screen.queryByRole('button', { name: 'Later' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Restart to apply' })).toBeTruthy();
    expect(api.invoke.mock.calls.some(([command]) => command === 'restart_after_update')).toBe(
      false,
    );
  });
});
