import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { t } from './i18n/index.svelte';
import { showToast } from './toast.svelte';

export type UpdatePhase =
  | 'idle'
  | 'checking'
  | 'up_to_date'
  | 'available'
  | 'waiting_for_idle'
  | 'downloading'
  | 'verifying'
  | 'installing'
  | 'restart_required'
  | 'error';
export interface UpdateSnapshot {
  revision: number;
  operationId: number;
  currentVersion: string;
  phase: UpdatePhase;
  autoUpdate: boolean;
  enabled: boolean;
  lastSuccessfulCheckAt: number | null;
  release: {
    version: string;
    notes: string;
    publishedAt: string | null;
    releaseUrl: string;
  } | null;
  downloadedBytes: number;
  totalBytes: number | null;
  error: { stage: string; code: string; retryable: boolean } | null;
  appliedVersion: string | null;
  pendingInstalledVersion: string | null;
}
export const updateState = $state<{
  snapshot: UpdateSnapshot | null;
  error: string | null;
  pending: boolean;
}>({
  snapshot: null,
  error: null,
  pending: false,
});
// Internal deduplication, never read by a template.
// eslint-disable-next-line svelte/prefer-svelte-reactivity
const notified = new Set<string>();

export function acceptUpdateSnapshot(snapshot: UpdateSnapshot): void {
  if (updateState.snapshot && snapshot.revision <= updateState.snapshot.revision) return;
  updateState.snapshot = snapshot;
  const phase = snapshot.phase;
  const version =
    phase === 'restart_required' ? snapshot.pendingInstalledVersion : snapshot.release?.version;
  if (version && (phase === 'available' || phase === 'restart_required')) {
    const key = `${phase}:${version}`;
    if (!notified.has(key)) {
      notified.add(key);
      showToast(t(phase === 'available' ? 'updates.found' : 'updates.installed', { version }));
    }
  }
  if (snapshot.appliedVersion && !notified.has(`applied:${snapshot.appliedVersion}`)) {
    notified.add(`applied:${snapshot.appliedVersion}`);
    showToast(t('updates.applied', { version: snapshot.appliedVersion }), { type: 'success' });
  }
}

export async function refreshUpdates(): Promise<void> {
  try {
    acceptUpdateSnapshot(await invoke<UpdateSnapshot>('get_update_state'));
    updateState.error = null;
  } catch {
    updateState.error = 'unavailable';
  }
}

export async function updateAction(
  command:
    | 'check_for_updates'
    | 'install_available_update'
    | 'restart_after_update'
    | 'set_update_settings',
  args?: Record<string, unknown>,
): Promise<void> {
  if (updateState.pending) return;
  updateState.pending = true;
  updateState.error = null;
  try {
    if (command === 'restart_after_update') {
      if (drafts.size) throw 'draft_active';
      await draftTail;
    }
    const snapshot = await invoke<UpdateSnapshot | null>(command, args);
    if (snapshot) acceptUpdateSnapshot(snapshot);
  } catch (error) {
    updateState.error = typeof error === 'string' ? error : 'unavailable';
  } finally {
    updateState.pending = false;
  }
}

// Aggregate dirty composers per window, serialize writes so cleanup cannot overtake edits.
// eslint-disable-next-line svelte/prefer-svelte-reactivity
const drafts = new Set<symbol>();
let draftTail: Promise<unknown> = Promise.resolve();
export function updateDraft(token: symbol, dirty: boolean): void {
  const before = drafts.size > 0;
  if (dirty) drafts.add(token);
  else drafts.delete(token);
  if (before === drafts.size > 0 || !isTauri()) return;
  const value = drafts.size > 0;
  draftTail = draftTail
    .catch(() => undefined)
    .then(() => invoke('set_update_draft', { dirty: value }));
  void draftTail.catch(() => undefined);
}

export function startUpdateSync(): () => void {
  if (!isTauri()) return () => {};
  let stopped = false;
  const unlisten = listen<UpdateSnapshot>('update-state-changed', (event) => {
    if (!stopped) acceptUpdateSnapshot(event.payload);
  });
  void unlisten
    .then(() => {
      if (!stopped) return refreshUpdates();
    })
    .catch(() => {
      if (!stopped) updateState.error = 'unavailable';
    });
  return () => {
    stopped = true;
    void unlisten.then((stop) => stop()).catch(() => undefined);
  };
}
