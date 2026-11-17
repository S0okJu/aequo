export type ToastType = 'success' | 'error' | 'info';

export interface Toast {
  id: number;
  type: ToastType;
  message: string;
}

const DEFAULT_DURATION_MS = 4000;

let nextId = 1;
// Svelte doesn't allow an exported `$state` binding to be reassigned from
// outside the module that declares it - mutate this array in place
// (push/splice) rather than ever writing `toasts = ...`.
export const toasts = $state<Toast[]>([]);

// Plain object, not Map/SvelteMap - this is internal timer bookkeeping, not
// reactive UI state, so it never needs Svelte's own change tracking.
const timers: Record<number, ReturnType<typeof setTimeout>> = {};

/**
 * Show a transient, global success/error/info toast. Error toasts never
 * auto-dismiss by default (matches the manual-dismiss convention AiPanel's
 * inline error card already used) - everything else clears itself after
 * `durationMs`.
 */
export function showToast(
  message: string,
  opts: { type?: ToastType; durationMs?: number } = {},
): number {
  const type = opts.type ?? 'info';
  const id = nextId++;
  toasts.push({ id, type, message });

  const durationMs = opts.durationMs ?? (type === 'error' ? 0 : DEFAULT_DURATION_MS);
  if (durationMs > 0) {
    timers[id] = setTimeout(() => dismissToast(id), durationMs);
  }
  return id;
}

export function dismissToast(id: number): void {
  clearTimeout(timers[id]);
  delete timers[id];
  const index = toasts.findIndex((toast) => toast.id === id);
  if (index !== -1) toasts.splice(index, 1);
}
