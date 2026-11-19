/** Account serialized UTF-8 data, not JS/DOM/image allocator overhead. */
const encoder = new TextEncoder();
export function serializedBytes(value: unknown): number {
  return encoder.encode(JSON.stringify(value)).byteLength;
}

interface Entry {
  bytes: number;
  evict?: () => void;
  shrink?: (maximumBytes: number) => number;
}

/** Shared LRU for optional data plus shrinkable reservations for visible lists. */
export class CacheBudget {
  private entries = new Map<string, Entry>();
  private maximum: number;

  constructor(bytes: number) {
    this.maximum = bytes;
  }
  get limit(): number {
    return this.maximum;
  }
  get bytes(): number {
    return [...this.entries.values()].reduce((sum, entry) => sum + entry.bytes, 0);
  }

  setLimit(bytes: number): void {
    if (!Number.isSafeInteger(bytes) || bytes < 0) throw new Error('Invalid cache budget');
    this.maximum = bytes;
    this.rebalance();
  }

  put(key: string, bytes: number, evict: () => void): boolean {
    this.checkSize(bytes);
    this.entries.delete(key);
    if (bytes > this.maximum) {
      evict();
      return false;
    }
    this.entries.set(key, { bytes, evict });
    this.rebalance();
    return this.entries.has(key);
  }

  hold(key: string, bytes: number, shrink: (maximumBytes: number) => number): void {
    this.checkSize(bytes);
    this.entries.set(key, { bytes, shrink });
    this.rebalance();
  }

  touch(key: string): void {
    const entry = this.entries.get(key);
    if (entry) {
      this.entries.delete(key);
      this.entries.set(key, entry);
    }
  }

  release(key: string): void {
    this.entries.delete(key);
  }

  private checkSize(bytes: number) {
    if (!Number.isSafeInteger(bytes) || bytes < 0) throw new Error('Invalid cache entry size');
  }

  private rebalance() {
    let excess = this.bytes - this.maximum;
    if (excess <= 0) return;
    // Optional icons/history go first, preserving current list context.
    for (const [key, entry] of this.entries) {
      if (excess <= 0) break;
      if (!entry.evict) continue;
      this.entries.delete(key);
      excess -= entry.bytes;
      entry.evict();
    }
    for (const entry of this.entries.values()) {
      if (excess <= 0) break;
      if (!entry.shrink) continue;
      const target = Math.max(0, entry.bytes - excess);
      const next = entry.shrink(target);
      this.checkSize(next);
      if (next > target) throw new Error('Cache owner failed to release its reservation');
      excess -= entry.bytes - next;
      entry.bytes = next;
    }
    if (excess > 0) throw new Error('Cache budget could not be enforced');
  }
}

export const managedCache = new CacheBudget(16 * 1024 * 1024);
