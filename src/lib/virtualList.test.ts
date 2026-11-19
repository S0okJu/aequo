import { describe, expect, it } from 'vitest';
import { MAX_RENDERED_ROWS, visibleRowIndices } from './virtualList';

describe('variable height row window', () => {
  it('covers the viewport and preserves offscreen focus within a 100-row cap', () => {
    const offsets = [0];
    for (let index = 0; index < 10000; index++) offsets.push(offsets.at(-1)! + 80 + (index % 170));
    for (const top of [0, 1000, 500000, offsets.at(-1)! - 1000]) {
      const rows = visibleRowIndices(offsets, top, 800, [0, 9999]);
      expect(rows.length).toBeLessThanOrEqual(MAX_RENDERED_ROWS);
      expect(rows).toContain(0);
      expect(rows).toContain(9999);
      for (let index = 0; index < 10000; index++) {
        if (offsets[index] < top + 800 && offsets[index + 1] > top) expect(rows).toContain(index);
      }
    }
    expect(visibleRowIndices([0], 0, 600)).toEqual([]);
  });
});
