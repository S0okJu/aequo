export const MAX_RENDERED_ROWS = 100;

/** First offset strictly greater than a pixel position. */
function upperBound(offsets: number[], value: number): number {
  let low = 0;
  let high = offsets.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (offsets[middle] <= value) low = middle + 1;
    else high = middle;
  }
  return low;
}

/** Include up to two offscreen pins (focus/scroll anchor) within the hard cap. */
export function visibleRowIndices(
  offsets: number[],
  top: number,
  height: number,
  pins: number[] = [],
): number[] {
  const count = Math.max(0, offsets.length - 1);
  if (!count) return [];
  const pinned = [...new Set(pins.filter((index) => index >= 0 && index < count))].slice(0, 2);
  const first = Math.max(0, Math.min(count - 1, upperBound(offsets, top) - 1));
  const start = Math.max(0, first - 5);
  const end = Math.min(
    count,
    upperBound(offsets, top + Math.max(1, height)) + 5,
    start + MAX_RENDERED_ROWS - pinned.length,
  );
  const indices = new Set(
    Array.from({ length: Math.max(0, end - start) }, (_, index) => start + index),
  );
  for (const pin of pinned) indices.add(pin);
  return [...indices].sort((a, b) => a - b);
}
