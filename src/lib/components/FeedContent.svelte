<script lang="ts">
  import {
    applyOverlay,
    extractPlainText,
    hashPlainText,
    reconcileAnnotations,
    selectionToOffsets,
  } from '$lib/annotation';
  import type { Annotation, AnnotationKind } from '$lib/types';
  import AnnotationPopover from './AnnotationPopover.svelte';
  import SelectionToolbar from './SelectionToolbar.svelte';
  import Icon from '$lib/icons/Icon.svelte';
  import { t } from '$lib/i18n/index.svelte';
  import { ICONS } from '$lib/icons';

  // Fixed, non-user-controlled markup (same path data as the Icon component's
  // "external-link" entry) - safe to inline into sanitizer-built HTML that
  // ends up passed to {@html}, since nothing here comes from the feed itself.
  const EXTERNAL_LINK_ICON_HTML = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" style="vertical-align:-1px;display:inline-block">${ICONS['external-link']}</svg>`;

  export interface CreateAnnotationInput {
    kind: AnnotationKind;
    contentHash: string;
    startOffset: number;
    endOffset: number;
    quotedText: string;
    color?: string | null;
    note?: string | null;
  }

  export type ReanchoredUpdate = Pick<
    Annotation,
    'id' | 'startOffset' | 'endOffset' | 'contentHash'
  >;

  interface Props {
    content: string | null;
    baseUrl: string;
    compact?: boolean;
    annotations?: Annotation[];
    onCreateAnnotation?: (input: CreateAnnotationInput) => void;
    onUpdateAnnotation?: (annotation: Annotation) => void;
    onDeleteAnnotation?: (id: number) => void;
    onReanchored?: (updates: ReanchoredUpdate[]) => void;
    onOrphanedChange?: (ids: number[]) => void;
    onAskAi?: (quotedText: string) => void;
  }

  let {
    content,
    baseUrl,
    compact = false,
    annotations = [],
    onCreateAnnotation,
    onUpdateAnnotation,
    onDeleteAnnotation,
    onReanchored,
    onOrphanedChange,
    onAskAi,
  }: Props = $props();

  const MAX_ASK_AI_QUOTE_LENGTH = 2000;

  const allowedTags = new Set([
    'A',
    'B',
    'BLOCKQUOTE',
    'BR',
    'CAPTION',
    'CODE',
    'DD',
    'DEL',
    'DIV',
    'DL',
    'DT',
    'EM',
    'H1',
    'H2',
    'H3',
    'H4',
    'HR',
    'I',
    'IMG',
    'LI',
    'OL',
    'P',
    'PRE',
    'S',
    'SPAN',
    'STRONG',
    'SUB',
    'SUP',
    'TABLE',
    'TBODY',
    'TD',
    'TFOOT',
    'TH',
    'THEAD',
    'TR',
    'U',
    'UL',
  ]);

  function safeUrl(value: string, allowMailto = false): string | null {
    try {
      const url = new URL(value, baseUrl);
      if (url.protocol === 'https:' || url.protocol === 'http:') return url.href;
      if (allowMailto && url.protocol === 'mailto:') return url.href;
    } catch {
      return null;
    }
    return null;
  }

  function videoLink(markup: string | null): string | null {
    if (!markup) return null;
    const parsed = new DOMParser().parseFromString(markup, 'text/html');
    const source = parsed.querySelector('iframe[src], video[src], video source[src], embed[src], object[data]');
    const rawUrl = source?.getAttribute('src') ?? source?.getAttribute('data');
    if (rawUrl) return safeUrl(rawUrl);
    const clipHandle = parsed.body.textContent?.match(/클립\s*\(@([A-Za-z0-9._-]+)\)/)?.[1];
    return clipHandle ? safeUrl(`https://clip.naver.com/@${clipHandle}`) : null;
  }

  function highlightCodeBlocks(root: ParentNode): void {
    const tokenPattern =
      /(\/\/[^\n]*|#[^\n]*|\/\*[\s\S]*?\*\/|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`|\b(?:as|async|await|bool|break|case|class|const|continue|def|else|enum|export|false|fn|for|from|function|if|impl|import|in|interface|let|match|mut|new|null|pub|return|self|static|struct|switch|this|throw|true|try|type|var|void|while)\b|\b\d+(?:\.\d+)?\b|\b[A-Za-z_$][\w$]*(?=\s*\())/g;
    const keywords =
      /^(as|async|await|bool|break|case|class|const|continue|def|else|enum|export|false|fn|for|from|function|if|impl|import|in|interface|let|match|mut|new|null|pub|return|self|static|struct|switch|this|throw|true|try|type|var|void|while)$/;

    root.querySelectorAll('pre code').forEach((code) => {
      const textNodes: Text[] = [];
      const collect = (node: Node) => {
        if (node.nodeType === Node.TEXT_NODE) textNodes.push(node as Text);
        else node.childNodes.forEach(collect);
      };
      collect(code);

      for (const textNode of textNodes) {
        const text = textNode.textContent ?? '';
        const fragment = document.createDocumentFragment();
        let cursor = 0;
        for (const match of text.matchAll(tokenPattern)) {
          const token = match[0];
          const index = match.index ?? 0;
          if (index > cursor)
            fragment.appendChild(document.createTextNode(text.slice(cursor, index)));
          const span = document.createElement('span');
          span.className = /^(\/\/|#|\/\*)/.test(token)
            ? 'code-comment'
            : /^["'`]/.test(token)
              ? 'code-string'
              : /^\d/.test(token)
                ? 'code-number'
                : keywords.test(token)
                  ? 'code-keyword'
                  : 'code-function';
          span.textContent = token;
          fragment.appendChild(span);
          cursor = index + token.length;
        }
        if (cursor < text.length) fragment.appendChild(document.createTextNode(text.slice(cursor)));
        textNode.replaceWith(fragment);
      }
    });
  }

  function sanitizedContent(markup: string | null): string {
    if (!markup) return `<p>${t('content.noPreviewPost')}</p>`;

    const source = new DOMParser().parseFromString(markup, 'text/html');
    const output = document.implementation.createHTMLDocument('');

    function copySafe(node: Node, parent: Node): void {
      if (node.nodeType === Node.TEXT_NODE) {
        const text = node.textContent ?? '';
        if (parent instanceof Element && parent.tagName === 'A') {
          parent.appendChild(output.createTextNode(text));
          return;
        }
        const clipMention = /(클립\s*\()(@[A-Za-z0-9._-]+)(\))/g;
        let cursor = 0;
        for (const match of text.matchAll(clipMention)) {
          const index = match.index ?? 0;
          if (index > cursor) parent.appendChild(output.createTextNode(text.slice(cursor, index)));
          parent.appendChild(output.createTextNode(match[1]));
          const href = safeUrl(`https://clip.naver.com/${match[2]}`);
          if (href) {
            const link = output.createElement('a');
            link.setAttribute('href', href);
            link.setAttribute('target', '_blank');
            link.setAttribute('rel', 'noopener noreferrer');
            link.textContent = match[2];
            parent.appendChild(link);
          } else {
            parent.appendChild(output.createTextNode(match[2]));
          }
          parent.appendChild(output.createTextNode(match[3]));
          cursor = index + match[0].length;
        }
        if (cursor < text.length) parent.appendChild(output.createTextNode(text.slice(cursor)));
        return;
      }
      if (!(node instanceof Element)) return;

      const tag = node.tagName;
      if (['SCRIPT', 'STYLE', 'NOSCRIPT', 'SVG', 'MATH'].includes(tag)) return;
      if (tag === 'IFRAME' || tag === 'VIDEO' || tag === 'EMBED' || tag === 'OBJECT') {
        const rawUrl = node.getAttribute('src') ?? node.getAttribute('data') ??
          (tag === 'VIDEO' ? node.querySelector('source[src]')?.getAttribute('src') : null);
        const href = rawUrl ? safeUrl(rawUrl) : null;
        if (href) {
          const link = output.createElement('a');
          link.setAttribute('href', href);
          link.setAttribute('target', '_blank');
          link.setAttribute('rel', 'noopener noreferrer');
          link.className = 'video-link';
          link.innerHTML = `${t('content.openVideoHost', { host: new URL(href).hostname })} ${EXTERNAL_LINK_ICON_HTML}`;
          parent.appendChild(link);
        }
        return;
      }
      if (!allowedTags.has(tag)) {
        node.childNodes.forEach((child) => copySafe(child, parent));
        return;
      }

      const clean = output.createElement(tag.toLowerCase());
      if (tag === 'A') {
        const href = node.getAttribute('href');
        const safeHref = href ? safeUrl(href, true) : null;
        if (safeHref) {
          clean.setAttribute('href', safeHref);
          clean.setAttribute('target', '_blank');
          clean.setAttribute('rel', 'noopener noreferrer');
        }
      } else if (tag === 'IMG') {
        const src = node.getAttribute('src');
        const safeSrc = src ? safeUrl(src) : null;
        if (!safeSrc) return;
        clean.setAttribute('src', safeSrc);
        for (const name of ['alt', 'title', 'width', 'height']) {
          const value = node.getAttribute(name);
          if (value && (!['width', 'height'].includes(name) || /^\d{1,4}$/.test(value))) {
            clean.setAttribute(name, value);
          }
        }
        clean.setAttribute('loading', 'lazy');
        clean.setAttribute('referrerpolicy', 'no-referrer');
      } else if (tag === 'TD' || tag === 'TH') {
        for (const name of ['colspan', 'rowspan']) {
          const value = node.getAttribute(name);
          if (value && /^\d{1,2}$/.test(value)) clean.setAttribute(name, value);
        }
      }

      parent.appendChild(clean);
      node.childNodes.forEach((child) => copySafe(child, clean));
    }

    source.body.childNodes.forEach((node) => copySafe(node, output.body));
    highlightCodeBlocks(output.body);
    return output.body.innerHTML.trim() || `<p>${t('content.noPreviewPost')}</p>`;
  }

  function textPreview(markup: string | null): string {
    if (!markup) return t('content.noPreview');
    const parsed = new DOMParser().parseFromString(markup, 'text/html');
    parsed.querySelectorAll('script, style, noscript').forEach((node) => node.remove());
    const text = (parsed.body.textContent ?? '').replace(/[\t\r\n ]+/g, ' ').trim();
    if (text.length <= 260) return text || t('content.noPreview');
    return `${text.slice(0, 260).trimEnd()}…`;
  }

  const html = $derived(compact ? '' : sanitizedContent(content));
  const preview = $derived(textPreview(content));
  const videoHref = $derived(videoLink(content));
  const videoLinkLabel = $derived(
    content && /클립\s*\(@[A-Za-z0-9._-]+\)/.test(content) ? t('content.openVideoChannel')
      : t('content.openVideo'),
  );

  // --- Annotation overlay rendering + self-healing reconciliation ---------

  let wrapperEl: HTMLDivElement | undefined = $state();
  let contentEl: HTMLDivElement | undefined = $state();
  let overlayGeneration = 0;
  let memoMarkers = $state<Array<{ id: number; top: number }>>([]);

  function recomputeMemoMarkerPositions() {
    if (!contentEl || !wrapperEl) return;
    const wrapperRect = wrapperEl.getBoundingClientRect();
    const marks = contentEl.querySelectorAll<HTMLElement>('[data-kind="memo"]');
    const next: Array<{ id: number; top: number }> = [];
    marks.forEach((mark) => {
      const id = Number(mark.getAttribute('data-annotation-id'));
      const rect = mark.getBoundingClientRect();
      next.push({ id, top: rect.top - wrapperRect.top });
    });
    memoMarkers = next;
  }

  // Re-renders the overlay whenever the sanitized HTML or the annotation
  // list changes. Guarded by a generation counter since hashing is async
  // and either input can change again before it resolves.
  $effect(() => {
    if (compact || !contentEl) return;
    void html;
    const currentAnnotations = annotations;
    const generation = ++overlayGeneration;
    const plainText = extractPlainText(contentEl);
    reconcileAnnotations(plainText, currentAnnotations).then((result) => {
      if (generation !== overlayGeneration || !contentEl) return;
      applyOverlay(contentEl, result.rendered);
      recomputeMemoMarkerPositions();
      onOrphanedChange?.(result.orphanedIds);
      if (result.toPersist.length > 0) onReanchored?.(result.toPersist);
    });
  });

  $effect(() => {
    if (compact || !contentEl || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => recomputeMemoMarkerPositions());
    observer.observe(contentEl);
    window.addEventListener('resize', recomputeMemoMarkerPositions);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', recomputeMemoMarkerPositions);
    };
  });

  // --- Selection -> floating toolbar ---------------------------------------

  let toolbarRect = $state<DOMRect | null>(null);
  let pendingSelection = $state<{ start: number; end: number; text: string } | null>(null);
  let truncationNotice = $state<DOMRect | null>(null);
  let popover = $state<{
    rect: DOMRect;
    kind: AnnotationKind;
    annotation: Annotation | null;
  } | null>(null);

  function clearSelectionUi() {
    toolbarRect = null;
    pendingSelection = null;
    window.getSelection()?.removeAllRanges();
  }

  // Cheap check on every selectionchange tick - only hides the toolbar when
  // the selection collapses. The expensive TreeWalker-based offset
  // computation only ever runs from handleSelectionFinished (mouseup/keyup),
  // never here (M15 performance requirement).
  //
  // While the annotation popover is open, this must be a no-op: focusing the
  // popover's note textarea moves focus off the article text, which collapses
  // `window.getSelection()` and fires `selectionchange` even though the user
  // hasn't abandoned anything. Clearing `pendingSelection` here would wipe
  // the selection a new memo is being created from before Save ever runs
  // `handleMemoSave`, silently dropping the note.
  function handleSelectionChange() {
    if (popover) return;
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed) {
      toolbarRect = null;
      pendingSelection = null;
    }
  }

  function handleSelectionFinished() {
    if (!contentEl) return;
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0) return;
    const range = selection.getRangeAt(0);
    if (!contentEl.contains(range.commonAncestorContainer)) return;
    const offsets = selectionToOffsets(contentEl, range);
    const text = selection.toString();
    if (!offsets || !text) return;
    pendingSelection = { start: offsets.start, end: offsets.end, text };
    toolbarRect = range.getBoundingClientRect();
  }

  $effect(() => {
    if (compact) return;
    document.addEventListener('selectionchange', handleSelectionChange);
    return () => document.removeEventListener('selectionchange', handleSelectionChange);
  });

  async function handleHighlight(color: string) {
    if (!pendingSelection || !contentEl) return;
    const { start, end, text } = pendingSelection;
    const contentHash = await hashPlainText(extractPlainText(contentEl));
    clearSelectionUi();
    onCreateAnnotation?.({
      kind: 'highlight',
      contentHash,
      startOffset: start,
      endOffset: end,
      quotedText: text,
      color,
    });
  }

  function handleMemoRequest() {
    if (!pendingSelection || !toolbarRect) return;
    popover = { rect: toolbarRect, kind: 'memo', annotation: null };
    toolbarRect = null;
  }

  function handleAskAiRequest() {
    if (!pendingSelection) return;
    const { text } = pendingSelection;
    const truncated = text.length > MAX_ASK_AI_QUOTE_LENGTH;
    const finalText = truncated ? text.slice(0, MAX_ASK_AI_QUOTE_LENGTH) : text;
    if (truncated && toolbarRect) {
      const rect = toolbarRect;
      truncationNotice = rect;
      setTimeout(() => {
        if (truncationNotice === rect) truncationNotice = null;
      }, 2500);
    }
    clearSelectionUi();
    onAskAi?.(finalText);
  }

  async function handleMemoSave(note: string) {
    if (popover?.annotation) {
      onUpdateAnnotation?.({ ...popover.annotation, note });
      popover = null;
      return;
    }
    if (!pendingSelection || !contentEl) return;
    const { start, end, text } = pendingSelection;
    const contentHash = await hashPlainText(extractPlainText(contentEl));
    popover = null;
    clearSelectionUi();
    onCreateAnnotation?.({
      kind: 'memo',
      contentHash,
      startOffset: start,
      endOffset: end,
      quotedText: text,
      note,
    });
  }

  function handleColorChange(color: string) {
    if (!popover?.annotation) return;
    onUpdateAnnotation?.({ ...popover.annotation, color });
    popover = null;
  }

  function handlePopoverDelete() {
    if (!popover?.annotation) return;
    onDeleteAnnotation?.(popover.annotation.id);
    popover = null;
  }

  function handleContentClick(event: MouseEvent) {
    const target = event.target as HTMLElement | null;
    const markEl = target?.closest<HTMLElement>('[data-annotation-id]');
    if (!markEl) return;
    const id = Number(markEl.getAttribute('data-annotation-id'));
    const found = annotations.find((candidate) => candidate.id === id);
    if (!found) return;
    popover = { rect: markEl.getBoundingClientRect(), kind: found.kind, annotation: found };
  }

  function handleMemoMarkerClick(id: number, event: MouseEvent) {
    const found = annotations.find((candidate) => candidate.id === id);
    if (!found) return;
    popover = {
      rect: (event.currentTarget as HTMLElement).getBoundingClientRect(),
      kind: found.kind,
      annotation: found,
    };
  }
</script>

{#if compact}
  <span class="content compact">{preview}</span>
  {#if videoHref}
    <!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
    <a class="video-link compact-video-link" href={videoHref} target="_blank" rel="noopener noreferrer" onclick={(event) => event.stopPropagation()}>{videoLinkLabel} <Icon name="external-link" size={12} /></a>
  {/if}
{:else}
  <div class="annotated-content" bind:this={wrapperEl}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="content"
      bind:this={contentEl}
      onmouseup={handleSelectionFinished}
      onkeyup={handleSelectionFinished}
      onclick={handleContentClick}
    >
      <!-- This HTML is rebuilt from an explicit element and URL allowlist. -->
      <!-- eslint-disable-next-line svelte/no-at-html-tags -->
      {@html html}
    </div>
    <div class="margin-rail">
      {#each memoMarkers as marker (marker.id)}
        <button
          type="button"
          class="memo-marker"
          style:top="{marker.top}px"
          aria-label={t('annotation.openMemo')}
          onclick={(event) => handleMemoMarkerClick(marker.id, event)}><Icon name="memo" size={14} /></button
        >
      {/each}
    </div>
  </div>

  {#if toolbarRect}
    <SelectionToolbar
      rect={toolbarRect}
      onHighlight={handleHighlight}
      onMemo={handleMemoRequest}
      onAskAi={handleAskAiRequest}
    />
  {/if}

  {#if truncationNotice}
    <div
      class="truncation-notice"
      style:top="{truncationNotice.top - 36}px"
      style:left="{truncationNotice.left}px"
    >
      {t('annotation.truncated')}
    </div>
  {/if}

  {#if popover}
    <AnnotationPopover
      rect={popover.rect}
      kind={popover.kind}
      annotation={popover.annotation}
      onSaveNote={handleMemoSave}
      onColorChange={handleColorChange}
      onDelete={handlePopoverDelete}
      onClose={() => (popover = null)}
    />
  {/if}
{/if}

<style>
  .content {
    color: var(--color-text);
    font-size: var(--text-reading);
    line-height: 1.7;
    word-break: keep-all;
    overflow-wrap: break-word;
  }

  .content.compact {
    display: -webkit-box;
    overflow: hidden;
    color: var(--color-text-secondary);
    font-size: var(--text-body);
    line-height: 1.55;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }

  .video-link { display: inline-block; margin-block: 6px; color: var(--color-accent); font-size: var(--text-small); font-weight: var(--weight-medium); text-decoration: none; }
  .video-link:hover { text-decoration: underline; }

  .annotated-content {
    position: relative;
    container-type: inline-size;
  }

  .margin-rail {
    position: absolute;
    top: 0;
    right: -34px;
    bottom: 0;
    width: 24px;
  }

  @container (max-width: 700px) {
    .margin-rail {
      display: none;
    }
  }

  .memo-marker {
    position: absolute;
    display: grid;
    width: 20px;
    height: 20px;
    place-items: center;
    border: 1px solid var(--color-border);
    border-radius: 50%;
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font-size: var(--text-caption);
    cursor: pointer;
  }

  .memo-marker:hover {
    border-color: var(--color-accent);
    color: var(--color-accent);
  }

  .truncation-notice {
    position: fixed;
    z-index: 51;
    padding: 5px 10px;
    border-radius: var(--radius-sm);
    background: var(--color-text);
    color: var(--color-bg);
    font-size: var(--text-small);
    white-space: nowrap;
  }

  .content :global(mark.aequo-hl) {
    border-radius: 2px;
    padding: 0 1px;
    cursor: pointer;
  }
  .content :global(mark.aequo-hl-yellow) {
    background: var(--hl-yellow);
  }
  .content :global(mark.aequo-hl-green) {
    background: var(--hl-green);
  }
  .content :global(mark.aequo-hl-blue) {
    background: var(--hl-blue);
  }
  .content :global(mark.aequo-hl-pink) {
    background: var(--hl-pink);
  }
  .content :global(mark.aequo-hl-purple) {
    background: var(--hl-purple);
  }

  .content :global(.aequo-memo-mark) {
    cursor: pointer;
    border-bottom: 1.5px dashed var(--color-text-tertiary);
  }

  .content :global(p) {
    margin: 0 0 1em;
    white-space: normal;
  }

  .content :global(p:last-child) {
    margin-bottom: 0;
  }

  .content :global(h1),
  .content :global(h2),
  .content :global(h3),
  .content :global(h4) {
    margin: 1.25em 0 0.5em;
    line-height: 1.3;
  }

  .content :global(img) {
    display: block;
    max-width: 100%;
    height: auto;
    margin: 14px auto;
    border-radius: var(--radius-md);
  }

  /* Feeds that omit width/height (common for RSS) give the browser nothing
     to size the box with before the image loads - reserve a sensible
     placeholder area so the surrounding text doesn't jump once it does. */
  .content :global(img:not([width])) {
    aspect-ratio: 16 / 9;
    background: var(--color-bg-secondary);
    object-fit: cover;
  }

  .content :global(pre) {
    max-width: 100%;
    overflow-x: auto;
    margin: 14px 0;
    padding: 14px 16px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg-secondary);
    color: var(--color-text);
    font:
      0.88em/1.65 ui-monospace,
      SFMono-Regular,
      Menlo,
      Consolas,
      monospace;
    white-space: pre;
    tab-size: 2;
  }

  .content :global(code) {
    padding: 0.12em 0.35em;
    border-radius: var(--radius-xs);
    background: var(--color-bg-secondary);
    font:
      0.9em ui-monospace,
      SFMono-Regular,
      Menlo,
      Consolas,
      monospace;
    white-space: normal;
  }

  .content :global(pre code) {
    padding: 0;
    background: transparent;
    white-space: inherit;
  }

  .content :global(.code-keyword) {
    color: var(--code-keyword);
    font-weight: var(--weight-bold);
  }
  .content :global(.code-string) {
    color: var(--code-string);
  }
  .content :global(.code-number) {
    color: var(--code-number);
  }
  .content :global(.code-comment) {
    color: var(--code-comment);
    font-style: italic;
  }
  .content :global(.code-function) {
    color: var(--code-function);
  }

  .content :global(blockquote) {
    margin: 1em 0;
    padding-left: 14px;
    border-left: 3px solid var(--color-border);
    color: var(--color-text-secondary);
  }

  .content :global(a) {
    color: var(--color-accent);
    overflow-wrap: anywhere;
  }
  .content :global(table) {
    display: block;
    max-width: 100%;
    overflow-x: auto;
    border-collapse: collapse;
  }
  .content :global(th),
  .content :global(td) {
    padding: 6px 9px;
    border: 1px solid var(--color-border);
    text-align: left;
  }

</style>
