import { describe, expect, it, vi } from 'vitest';
import { CacheBudget, serializedBytes } from './cacheBudget';

describe('shared cache budget', () => {
  it('evicts least-recently-used optional data before shrinking the visible list', () => {
    const cache = new CacheBudget(100);
    const first = vi.fn();
    const second = vi.fn();
    const shrink = vi.fn((maximum: number) => maximum);
    cache.hold('list', 40, shrink);
    cache.put('icon:1', 25, first);
    cache.put('ai:1', 25, second);
    cache.touch('icon:1');
    cache.put('icon:2', 25, vi.fn());
    expect(second).toHaveBeenCalledOnce();
    expect(first).not.toHaveBeenCalled();
    expect(shrink).not.toHaveBeenCalled();
    expect(cache.bytes).toBe(90);
    cache.setLimit(30);
    expect(first).toHaveBeenCalledOnce();
    expect(shrink).toHaveBeenCalledWith(30);
    expect(cache.bytes).toBe(30);
  });

  it('accounts replacement, oversized admission and owner disposal without accumulating bytes', () => {
    const cache = new CacheBudget(10);
    const evict = vi.fn();
    expect(cache.put('one', 8, evict)).toBe(true);
    expect(cache.put('one', 9, evict)).toBe(true);
    expect(cache.bytes).toBe(9);
    expect(cache.put('one', 11, evict)).toBe(false);
    expect(cache.bytes).toBe(0);
    cache.hold('list', 5, (maximum) => maximum);
    cache.release('list');
    expect(cache.bytes).toBe(0);
    expect(() => cache.setLimit(-1)).toThrow();
    expect(serializedBytes('한')).toBe(5); // JSON quotes + three UTF-8 bytes
  });
});
