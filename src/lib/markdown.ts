// Minimal, escape-first Markdown for AI answers: paragraphs, headings,
// lists, fenced code, inline code/bold/italic and http(s) links. Everything
// is HTML-escaped before any tag is added, so model output can't inject
// markup (moved unchanged from AiPanel.svelte).

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (char) => {
    const entities: Record<string, string> = {
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      '"': '&quot;',
      "'": '&#39;',
    };
    return entities[char];
  });
}

export interface CitationSource {
  itemId: number;
  title?: string | null;
}

interface CitationContext {
  citations: ReadonlyMap<number, number | CitationSource>;
  sourceLabel: string;
  referenced: Map<number, CitationSource>;
  /** `@article` labels still present in a sent message, keyed by the exact label text typed in the composer. */
  mentionLabels?: ReadonlyMap<string, number>;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Re-applies the composer's `@article` highlight to a sent message, so a reference looks the same before and after sending. */
function applyMentionLabels(part: string, labels: ReadonlyMap<string, number> | undefined): string {
  if (!labels || labels.size === 0) return part;
  const escaped = [...labels].map(([label, itemId]) => [escapeHtml(label), itemId] as const);
  const pattern = new RegExp(escaped.map(([label]) => escapeRegExp(label)).join('|'), 'g');
  return part.replace(pattern, (match) => {
    const itemId = escaped.find(([label]) => label === match)?.[1];
    if (itemId === undefined) return match;
    return `<a class="mention-highlight" href="#aequo-source-${itemId}" data-article-id="${itemId}">${match}</a>`;
  });
}

function inlineMarkdown(value: string, context: CitationContext): string {
  let safe = escapeHtml(value);
  safe = safe.replace(/`([^`]+)`/g, '<code>$1</code>');
  safe = safe.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  safe = safe.replace(/__([^_]+)__/g, '<strong>$1</strong>');
  safe = safe.replace(/\*([^*]+)\*/g, '<em>$1</em>');
  safe = safe.replace(/_([^_]+)_/g, '<em>$1</em>');
  safe = safe.replace(
    /(^|\s)(https?:\/\/[^\s<]+)/g,
    '$1<a href="$2" target="_blank" rel="noopener noreferrer">$2</a>',
  );
  safe = safe.replace(
    /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
    (_match, label: string, url: string) =>
      `<a href="${url}" target="_blank" rel="noopener noreferrer">${label}</a>`,
  );
  let protectedDepth = 0;
  return safe
    .split(/(<[^>]+>)/g)
    .map((part) => {
      if (/^<(a|code)\b/.test(part)) protectedDepth++;
      else if (/^<\/(a|code)>/.test(part)) protectedDepth = Math.max(0, protectedDepth - 1);
      if (part.startsWith('<') || protectedDepth) return part;
      const withCitations = part.replace(/\[(\d+)\]/g, (match, number: string) => {
        const index = Number(number);
        const value = context.citations.get(index);
        if (value === undefined) return match;
        const source = typeof value === 'number' ? { itemId: value } : value;
        context.referenced.set(index, source);
        const label = `${context.sourceLabel} ${index}${source.title ? `: ${source.title}` : ''}`;
        return `<a class="citation" href="#aequo-source-${source.itemId}" data-article-id="${source.itemId}" title="${escapeHtml(source.title ?? label)}" aria-label="${escapeHtml(label)}">${index}</a>`;
      });
      return applyMentionLabels(withCitations, context.mentionLabels);
    })
    .join('');
}

export function renderMarkdown(
  markdown: string,
  citations: ReadonlyMap<number, number | CitationSource> = new Map(),
  options: {
    sourceLabel?: string;
    includeSources?: boolean;
    /** Shown when the answer cites no `[N]` marker, so a reader still sees what was in context. */
    fallbackSources?: CitationSource[];
    /** `@article` labels (typically from the sender's own message) to keep highlighted after sending. */
    mentionLabels?: ReadonlyMap<string, number>;
  } = {},
): string {
  const context: CitationContext = {
    citations,
    sourceLabel: options.sourceLabel ?? 'Source',
    referenced: new Map(),
    mentionLabels: options.mentionLabels,
  };
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  const blocks: string[] = [];
  let paragraph: string[] = [];
  let list: string[] = [];
  let listKind: 'ul' | 'ol' | null = null;
  let code: string[] = [];
  let inCode = false;
  let quote: string[] = [];
  let table: string[][] = [];
  let tableAligns: (string | null)[] = [];

  const flushParagraph = () => {
    if (paragraph.length)
      blocks.push(`<p>${paragraph.map((line) => inlineMarkdown(line, context)).join('<br>')}</p>`);
    paragraph = [];
  };
  const flushList = () => {
    if (listKind && list.length)
      blocks.push(
        `<${listKind}>${list.map((item) => `<li>${inlineMarkdown(item, context)}</li>`).join('')}</${listKind}>`,
      );
    list = [];
    listKind = null;
  };
  const flushQuote = () => {
    if (quote.length)
      blocks.push(
        `<blockquote>${quote.map((line) => `<p>${inlineMarkdown(line, context)}</p>`).join('')}</blockquote>`,
      );
    quote = [];
  };
  const flushTable = () => {
    if (table.length === 0) return;
    const [header, ...body] = table;
    const align = (i: number) => (tableAligns[i] ? ` style="text-align:${tableAligns[i]}"` : '');
    const headRow = `<tr>${header.map((cell, i) => `<th${align(i)}>${inlineMarkdown(cell, context)}</th>`).join('')}</tr>`;
    const bodyRows = body
      .map(
        (row) =>
          `<tr>${row.map((cell, i) => `<td${align(i)}>${inlineMarkdown(cell, context)}</td>`).join('')}</tr>`,
      )
      .join('');
    blocks.push(
      `<div class="table-scroll"><table><thead>${headRow}</thead><tbody>${bodyRows}</tbody></table></div>`,
    );
    table = [];
    tableAligns = [];
  };
  const flushAll = () => {
    flushParagraph();
    flushList();
    flushQuote();
    flushTable();
  };

  const tableRow = (line: string): string[] => {
    let trimmed = line.trim();
    if (trimmed.startsWith('|')) trimmed = trimmed.slice(1);
    if (trimmed.endsWith('|')) trimmed = trimmed.slice(0, -1);
    return trimmed.split('|').map((cell) => cell.trim());
  };
  const isTableSeparator = (line: string): boolean => {
    const cells = tableRow(line);
    return cells.length > 0 && cells.every((cell) => /^:?-+:?$/.test(cell));
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s*```/.test(line)) {
      flushAll();
      if (inCode) {
        blocks.push(`<pre><code>${escapeHtml(code.join('\n'))}</code></pre>`);
        code = [];
        inCode = false;
      } else {
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      code.push(line);
      continue;
    }
    if (!line.trim()) {
      flushAll();
      continue;
    }

    if (table.length === 0 && /^\s*\|.*\|\s*$/.test(line) && isTableSeparator(lines[i + 1] ?? '')) {
      flushParagraph();
      flushList();
      flushQuote();
      tableAligns = tableRow(lines[i + 1]).map((cell) => {
        const left = cell.startsWith(':');
        const right = cell.endsWith(':');
        return left && right ? 'center' : right ? 'right' : left ? 'left' : null;
      });
      table = [tableRow(line)];
      i++;
      continue;
    }
    if (table.length > 0) {
      if (/^\s*\|.*\|\s*$/.test(line)) {
        table.push(tableRow(line));
        continue;
      }
      flushTable();
    }

    const quoted = line.match(/^\s{0,3}>\s?(.*)$/);
    if (quoted) {
      flushParagraph();
      flushList();
      quote.push(quoted[1]);
      continue;
    }
    flushQuote();

    const heading = line.match(/^\s{0,3}(#{1,4})\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      const level = heading[1].length;
      blocks.push(`<h${level}>${inlineMarkdown(heading[2], context)}</h${level}>`);
      continue;
    }

    const bullet = line.match(/^\s*[-*+]\s+(.+)$/);
    const numbered = line.match(/^\s*\d+[.)]\s+(.+)$/);
    const item = bullet?.[1] ?? numbered?.[1];
    const kind = bullet ? 'ul' : numbered ? 'ol' : null;
    if (item && kind) {
      flushParagraph();
      if (listKind && listKind !== kind) flushList();
      listKind = kind;
      list.push(item);
      continue;
    }

    flushList();
    paragraph.push(line);
  }

  if (inCode) blocks.push(`<pre><code>${escapeHtml(code.join('\n'))}</code></pre>`);
  flushAll();
  // A one-line status update ("let me check that article") isn't actually
  // informed by the source yet, so a fallback source list under it would
  // just be noise - real `[N]` citations always show regardless of length.
  const substantive = markdown.trim().length > 40;
  if (
    options.includeSources &&
    (context.referenced.size || (substantive && options.fallbackSources?.length))
  ) {
    const sources = context.referenced.size
      ? [...context.referenced].sort(([a], [b]) => a - b)
      : (options.fallbackSources ?? []).map((source, index) => [index + 1, source] as const);
    blocks.push(
      `<div class="answer-sources" aria-label="${escapeHtml(context.sourceLabel)}"><span class="sources-label">${escapeHtml(context.sourceLabel)}</span>${sources
        .map(
          ([number, source]) =>
            `<a class="answer-source" href="#aequo-source-${source.itemId}" data-article-id="${source.itemId}" title="${escapeHtml(source.title ?? `${context.sourceLabel} ${number}`)}"><span class="source-number">${number}</span> <span class="source-title">${escapeHtml(source.title ?? `${context.sourceLabel} ${number}`)}</span></a>`,
        )
        .join('')}</div>`,
    );
  }
  return blocks.join('');
}
