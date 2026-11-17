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

function inlineMarkdown(value: string): string {
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
  return safe;
}

export function renderMarkdown(markdown: string): string {
  const lines = markdown.replace(/\r\n?/g, '\n').split('\n');
  const blocks: string[] = [];
  let paragraph: string[] = [];
  let list: string[] = [];
  let listKind: 'ul' | 'ol' | null = null;
  let code: string[] = [];
  let inCode = false;

  const flushParagraph = () => {
    if (paragraph.length) blocks.push(`<p>${paragraph.map(inlineMarkdown).join('<br>')}</p>`);
    paragraph = [];
  };
  const flushList = () => {
    if (listKind && list.length)
      blocks.push(
        `<${listKind}>${list.map((item) => `<li>${inlineMarkdown(item)}</li>`).join('')}</${listKind}>`,
      );
    list = [];
    listKind = null;
  };

  for (const line of lines) {
    if (/^\s*```/.test(line)) {
      flushParagraph();
      flushList();
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
      flushParagraph();
      flushList();
      continue;
    }

    const heading = line.match(/^\s{0,3}(#{1,4})\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      const level = heading[1].length;
      blocks.push(`<h${level}>${inlineMarkdown(heading[2])}</h${level}>`);
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
  flushParagraph();
  flushList();
  return blocks.join('');
}
