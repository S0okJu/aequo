// Text anchoring and overlay rendering for highlights/memos (see
// docs/READER_ANNOTATIONS_PLAN.md §2). Pure DOM/text utilities only -
// no Tauri IPC here; callers (FeedContent/PostReader) own persistence.

import DiffMatchPatch from 'diff-match-patch';
import { t, type MessageKey } from './i18n/index.svelte';
import type { Annotation } from './types';

const OVERLAY_ATTRIBUTE = 'data-aequo-overlay';
const OVERLAY_SELECTOR = `[${OVERLAY_ATTRIBUTE}]`;

// match_main's own internal fuzziness budget (fraction of pattern
// length it's willing to search around), not the acceptance
// threshold - see relocateByFuzzyMatch for the confidence check that
// actually decides whether a fuzzy match is trusted.
const FUZZY_SEARCH_THRESHOLD = 0.3;
const FUZZY_SEARCH_DISTANCE = 1000;
// Minimum similarity (1 - editDistance / maxLength) a fuzzy match must
// reach before it's trusted enough to relocate an annotation.
const FUZZY_CONFIDENCE_THRESHOLD = 0.8;

function ownerDocument(node: Node): Document {
  return node.nodeType === Node.DOCUMENT_NODE
    ? (node as Document)
    : (node.ownerDocument ?? document);
}

/** Concatenates every text node under `root`, in document order. This is the
 * coordinate space every offset in this module is expressed in. */
export function extractPlainText(root: Node): string {
  const walker = ownerDocument(root).createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let text = '';
  let node: Node | null;
  while ((node = walker.nextNode())) {
    text += node.textContent ?? '';
  }
  return text;
}

export async function hashPlainText(text: string): Promise<string> {
  const data = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest('SHA-256', data);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, '0'))
    .join('');
}

/** Converts a `[start, end)` plain-text offset pair into a `Range` over `root`'s
 * current text nodes. Returns null if the offsets fall outside the text. */
export function offsetsToRange(root: Node, start: number, end: number): Range | null {
  if (end <= start) return null;
  const doc = ownerDocument(root);
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let consumed = 0;
  let startNode: Text | null = null;
  let startNodeOffset = 0;
  let endNode: Text | null = null;
  let endNodeOffset = 0;
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const text = node as Text;
    const length = text.length;
    if (startNode === null && consumed + length >= start) {
      startNode = text;
      startNodeOffset = start - consumed;
    }
    if (consumed + length >= end) {
      endNode = text;
      endNodeOffset = end - consumed;
      break;
    }
    consumed += length;
  }
  if (!startNode || !endNode) return null;
  const range = doc.createRange();
  range.setStart(startNode, startNodeOffset);
  range.setEnd(endNode, endNodeOffset);
  return range;
}

/** Converts a live `Range` (e.g. from `window.getSelection()`) back into
 * plain-text offsets relative to `root`. Only ranges whose boundary points
 * sit directly inside a text node under `root` are supported - selections
 * are text-only per docs/READER_ANNOTATIONS_PLAN.md §1 ("비목표"). */
export function selectionToOffsets(
  root: Node,
  range: Range,
): { start: number; end: number } | null {
  const doc = ownerDocument(root);
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let consumed = 0;
  let start: number | null = null;
  let end: number | null = null;
  let node: Node | null;
  while ((node = walker.nextNode())) {
    if (start === null && node === range.startContainer) {
      start = consumed + range.startOffset;
    }
    if (node === range.endContainer) {
      end = consumed + range.endOffset;
      break;
    }
    consumed += (node.textContent ?? '').length;
  }
  if (start === null || end === null || end <= start) return null;
  return { start, end };
}

/** Removes every overlay element previously inserted by `applyOverlay`,
 * restoring plain text nodes, and merges adjacent text nodes back together
 * so offset math stays correct on the next pass. Always call before
 * computing plain text/hashes against `root` - a leftover overlay would
 * not change the text content, but repeated apply/remove cycles without
 * this would otherwise fragment text nodes indefinitely. */
export function removeOverlay(root: Element): void {
  root.querySelectorAll(OVERLAY_SELECTOR).forEach((el) => {
    const parent = el.parentNode;
    if (!parent) return;
    while (el.firstChild) parent.insertBefore(el.firstChild, el);
    parent.removeChild(el);
  });
  root.normalize();
}

/** Wraps the text in `[start, end)` under `root`, splitting text nodes at the
 * boundaries as needed and wrapping each resulting segment with an element
 * from `makeWrapper`. Mutates from the last segment backwards so splitting
 * a later node never invalidates an earlier segment's reference. */
function wrapRange(root: Element, start: number, end: number, makeWrapper: () => Element): void {
  const doc = root.ownerDocument;
  const walker = doc.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const segments: Array<{ node: Text; from: number; to: number }> = [];
  let consumed = 0;
  let node: Node | null;
  while ((node = walker.nextNode())) {
    const text = node as Text;
    const length = text.length;
    const nodeStart = consumed;
    const nodeEnd = consumed + length;
    if (nodeEnd > start && nodeStart < end) {
      segments.push({
        node: text,
        from: Math.max(0, start - nodeStart),
        to: Math.min(length, end - nodeStart),
      });
    }
    consumed = nodeEnd;
    if (consumed >= end) break;
  }
  for (let i = segments.length - 1; i >= 0; i--) {
    const { node: text, from, to } = segments[i];
    if (to < text.length) text.splitText(to);
    const middle = from > 0 ? text.splitText(from) : text;
    const wrapper = makeWrapper();
    middle.parentNode?.replaceChild(wrapper, middle);
    wrapper.appendChild(middle);
  }
}

export interface ApplyOverlayOptions {
  /** Highlight colors known to the UI (see docs/READER_ANNOTATIONS_PLAN.md §4.1). */
  fallbackColor?: string;
}

const MEMO_PREVIEW_LENGTH = 60;

/** Accessible name for an overlay mark - names its kind, color (highlight)
 * or a note preview (memo) so a keyboard/screen-reader user can tell what's
 * there without first opening it (docs/UNIFIED_UX_MOTION_PLAN.md §7.5 A2). */
function accessibleName(annotation: Annotation, fallbackColor: string): string {
  if (annotation.kind === 'highlight') {
    const color = annotation.color ?? fallbackColor;
    return t('annotation.mark.highlight', { color: t(`color.${color}` as MessageKey) });
  }
  const note = annotation.note ?? '';
  const preview =
    note.length > MEMO_PREVIEW_LENGTH ? `${note.slice(0, MEMO_PREVIEW_LENGTH)}…` : note;
  return t('annotation.mark.memo', { note: preview });
}

/** Renders `annotations` as `<mark>`/`<span>` overlays over `root`'s text.
 * Idempotent - always clears any previous overlay first, so it's safe to
 * call again whenever `annotations` changes without `content` changing. */
export function applyOverlay(
  root: Element,
  annotations: Annotation[],
  options: ApplyOverlayOptions = {},
): void {
  removeOverlay(root);
  const fallbackColor = options.fallbackColor ?? 'yellow';
  const sorted = [...annotations].sort((a, b) => a.startOffset - b.startOffset);
  for (const annotation of sorted) {
    wrapRange(root, annotation.startOffset, annotation.endOffset, () => {
      const wrapper = root.ownerDocument.createElement(
        annotation.kind === 'highlight' ? 'mark' : 'span',
      );
      wrapper.setAttribute(OVERLAY_ATTRIBUTE, '');
      wrapper.setAttribute('data-annotation-id', String(annotation.id));
      wrapper.setAttribute('data-kind', annotation.kind);
      // Keyboard/screen-reader access to an *existing* mark (opening a new
      // one still needs a mouse/trackpad selection - docs/UNIFIED_UX_MOTION_PLAN.md
      // §7.5, §11 scope note). The click handler already reads these same
      // data-* attributes; Enter/Space on this element is wired alongside it.
      wrapper.setAttribute('tabindex', '0');
      wrapper.setAttribute('role', 'button');
      wrapper.setAttribute('aria-label', accessibleName(annotation, fallbackColor));
      wrapper.className =
        annotation.kind === 'highlight'
          ? `aequo-hl aequo-hl-${annotation.color ?? fallbackColor}`
          : 'aequo-memo-mark';
      return wrapper;
    });
  }
}

function allIndexesOf(haystack: string, needle: string): number[] {
  const indexes: number[] = [];
  let from = 0;
  for (;;) {
    const index = haystack.indexOf(needle, from);
    if (index === -1) break;
    indexes.push(index);
    from = index + 1;
  }
  return indexes;
}

function relocateByExactMatch(
  plainText: string,
  annotation: Annotation,
): { start: number; end: number } | null {
  const needle = annotation.quotedText;
  if (!needle) return null;
  const matches = allIndexesOf(plainText, needle);
  if (matches.length === 0) return null;
  // Multiple matches: pick the one closest to the stored absolute offset.
  // The doc's §2.2 describes picking by the match's relative position
  // (ratio) in the old document, but the schema stores no original
  // document length to compute that ratio from - absolute proximity to
  // the stored offset is the closest schema-free substitute (resolved in
  // docs/READER_ANNOTATIONS_PLAN.md open questions during planning).
  let best = matches[0];
  let bestDistance = Math.abs(matches[0] - annotation.startOffset);
  for (const candidate of matches.slice(1)) {
    const distance = Math.abs(candidate - annotation.startOffset);
    if (distance < bestDistance) {
      best = candidate;
      bestDistance = distance;
    }
  }
  return { start: best, end: best + needle.length };
}

function similarity(a: string, b: string): number {
  if (a.length === 0 && b.length === 0) return 1;
  const dmp = new DiffMatchPatch();
  const diffs = dmp.diff_main(a, b);
  const distance = dmp.diff_levenshtein(diffs);
  return 1 - distance / Math.max(a.length, b.length);
}

function relocateByFuzzyMatch(
  plainText: string,
  annotation: Annotation,
): { start: number; end: number } | null {
  const needle = annotation.quotedText;
  if (!needle || plainText.length === 0) return null;
  const dmp = new DiffMatchPatch();
  dmp.Match_Threshold = FUZZY_SEARCH_THRESHOLD;
  dmp.Match_Distance = FUZZY_SEARCH_DISTANCE;
  const anchor = Math.min(annotation.startOffset, plainText.length - 1);
  // match_bitap_ (the actual fuzzy search behind match_main) packs the
  // pattern into a 32-bit alphabet and throws for anything longer than
  // Match_MaxBits, so only the lead portion of a long quote is used to
  // *locate* a candidate position; the confidence check below still
  // compares the full quoted text against the full-length candidate.
  const search = needle.length > dmp.Match_MaxBits ? needle.slice(0, dmp.Match_MaxBits) : needle;
  const loc = dmp.match_main(plainText, search, anchor);
  if (loc === -1) return null;

  const end = Math.min(plainText.length, loc + needle.length);
  const candidate = plainText.slice(loc, end);
  if (similarity(candidate, needle) < FUZZY_CONFIDENCE_THRESHOLD) return null;
  return { start: loc, end };
}

export interface ReconcileOutcome {
  /** Annotations ready to render, with offsets/hash already updated for any
   * that were relocated this pass. */
  rendered: Annotation[];
  /** Relocated annotations the caller must persist via `updateAnnotation`. */
  toPersist: Array<Pick<Annotation, 'id' | 'startOffset' | 'endOffset' | 'contentHash'>>;
  /** Annotations that could not be placed in the current text at all - never
   * rendered over the wrong text (fail-closed per §7), but kept here so a
   * memo list can show a "couldn't find this" badge instead of silently
   * dropping the note. */
  orphanedIds: number[];
}

/** Self-healing anchoring pass (docs/READER_ANNOTATIONS_PLAN.md §2.2): if
 * `plainText`'s hash still matches an annotation's stored hash, it's
 * unchanged. Otherwise try an exact `quotedText` search, then a
 * diff-match-patch fuzzy search; only accept a relocation that clears the
 * confidence threshold, and give up (orphan) only when both fail. */
export async function reconcileAnnotations(
  plainText: string,
  annotations: Annotation[],
): Promise<ReconcileOutcome> {
  if (annotations.length === 0) {
    return { rendered: [], toPersist: [], orphanedIds: [] };
  }

  const currentHash = await hashPlainText(plainText);
  const rendered: Annotation[] = [];
  const toPersist: ReconcileOutcome['toPersist'] = [];
  const orphanedIds: number[] = [];

  for (const annotation of annotations) {
    if (annotation.contentHash === currentHash) {
      rendered.push(annotation);
      continue;
    }

    const relocated =
      relocateByExactMatch(plainText, annotation) ?? relocateByFuzzyMatch(plainText, annotation);
    if (!relocated) {
      orphanedIds.push(annotation.id);
      continue;
    }

    rendered.push({
      ...annotation,
      startOffset: relocated.start,
      endOffset: relocated.end,
      contentHash: currentHash,
    });
    toPersist.push({
      id: annotation.id,
      startOffset: relocated.start,
      endOffset: relocated.end,
      contentHash: currentHash,
    });
  }

  return { rendered, toPersist, orphanedIds };
}
