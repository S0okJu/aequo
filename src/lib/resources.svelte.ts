import { listen } from '@tauri-apps/api/event';
import { getResourceSettings, setResourceSettings, setResourceVisibility } from './api';
import presets from './resourcePresets.json';
import type { ResourceMode, ResourceSettings, ResourceStatus } from './types';
import { managedCache } from './cacheBudget';

export function resourcePreset(mode: Exclude<ResourceMode, 'custom'>): ResourceSettings {
  return { ...presets[mode] } as ResourceSettings;
}

export const resourceState = $state<{
  status: ResourceStatus | null;
  loading: boolean;
  error: string | null;
}>({ status: null, loading: true, error: null });

/** Conservative until the backend confirms persisted settings. */
const fallback = resourcePreset('light');
export function resourceSettings(): ResourceSettings {
  return resourceState.status?.snapshot.settings ?? fallback;
}

export function acceptResourceStatus(status: ResourceStatus): void {
  if (resourceState.status && status.snapshot.revision < resourceState.status.snapshot.revision)
    return;
  resourceState.status = status;
  managedCache.setLimit(status.snapshot.settings.cacheBudgetMib * 1024 * 1024);
  resourceState.loading = false;
  resourceState.error = null;
}

export async function refreshResourceSettings(): Promise<ResourceStatus> {
  try {
    const status = await getResourceSettings();
    acceptResourceStatus(status);
    // An event may have published a newer revision while the query was in flight.
    return resourceState.status!;
  } catch (error) {
    resourceState.loading = false;
    resourceState.error = String(error);
    throw error;
  }
}

let saveTail: Promise<unknown> = Promise.resolve();
/** Serialize full snapshots so rapid edits retain the last user's intent. */
export function saveResourceSettings(settings: ResourceSettings): Promise<ResourceStatus> {
  const copy = { ...settings };
  const next = saveTail
    .catch(() => undefined)
    .then(async () => {
      const current = resourceState.status ?? (await refreshResourceSettings());
      try {
        const result = await setResourceSettings(current.snapshot.revision, copy);
        acceptResourceStatus(result);
        return result;
      } catch (error) {
        await refreshResourceSettings().catch(() => undefined);
        throw error;
      }
    });
  saveTail = next;
  return next;
}

/** Owned by the root layout; no polling timer or immortal duplicate listener. */
export function startResourceSync(): () => void {
  let stopped = false;
  const unlisten = listen<ResourceStatus>('resource-settings-changed', (event) => {
    if (!stopped) acceptResourceStatus(event.payload);
  });
  unlisten.catch((error) => {
    if (!stopped) resourceState.error = String(error);
  });
  // Subscribe first, then read, preventing a missed update between the two.
  void unlisten
    .then(() => {
      if (!stopped) return refreshResourceSettings();
    })
    .catch(() => undefined);
  const visibility = () => {
    void setResourceVisibility(document.visibilityState === 'hidden').catch((error) => {
      if (!stopped) resourceState.error = String(error);
    });
  };
  document.addEventListener('visibilitychange', visibility);
  visibility();
  return () => {
    stopped = true;
    document.removeEventListener('visibilitychange', visibility);
    void unlisten.then((stop) => stop()).catch(() => undefined);
  };
}
