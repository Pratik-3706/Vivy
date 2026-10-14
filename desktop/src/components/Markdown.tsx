import React from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';

/**
 * Small Markdown renderer for Vivy's replies: paragraphs, headings, quotes,
 * lists, code blocks and inline bold / italic / code / links. Builds React
 * elements directly (no HTML strings), so model output can't inject markup.
 */
export const Markdown: React.FC<{ text: string; className?: string }> = ({ text, className }) => (
  <div className={`md ${className ?? ''}`}>{renderBlocks(text)}</div>
);

type Block =
  | { kind: 'p'; lines: string[] }
  | { kind: 'h'; level: number; text: string }
  | { kind: 'quote'; lines: string[] }
  | { kind: 'ul' | 'ol'; items: string[] }
  | { kind: 'code'; text: string };

function parseBlocks(src: string): Block[] {
  const lines = src.replace(/\r\n/g, '\n').split('\n');
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    if (trimmed.startsWith('```')) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith('```')) body.push(lines[i++]);
      i++; // closing fence (or end of text while still streaming)
      blocks.push({ kind: 'code', text: body.join('\n') });
      continue;
    }
    if (!trimmed) {
      i++;
      continue;
    }
    const heading = /^(#{1,4})\s+(.*)$/.exec(trimmed);
    if (heading) {
      blocks.push({ kind: 'h', level: heading[1].length, text: heading[2] });
      i++;
      continue;
    }
    if (trimmed.startsWith('>')) {
      const quoted: string[] = [];
      while (i < lines.length && lines[i].trim().startsWith('>')) {
        quoted.push(lines[i].trim().replace(/^>\s?/, ''));
        i++;
      }
      blocks.push({ kind: 'quote', lines: quoted });
      continue;
    }
    const bullet = /^[-*•]\s+/;
    const numbered = /^\d+[.)]\s+/;
    if (bullet.test(trimmed) || numbered.test(trimmed)) {
      const kind = bullet.test(trimmed) ? 'ul' : 'ol';
      const re = kind === 'ul' ? bullet : numbered;
      const items: string[] = [];
      while (i < lines.length && re.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(re, ''));
        i++;
      }
      blocks.push({ kind, items });
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() && !/^(```|#{1,4}\s|>|[-*•]\s|\d+[.)]\s)/.test(lines[i].trim())) {
      para.push(lines[i].trim());
      i++;
    }
    blocks.push({ kind: 'p', lines: para });
  }
  return blocks;
}

function renderBlocks(src: string): React.ReactNode[] {
  return parseBlocks(src).map((b, i) => {
    switch (b.kind) {
      case 'code':
        return (
          <pre key={i} className="md-code">
            <code>{b.text}</code>
          </pre>
        );
      case 'h':
        return (
          <p key={i} className={`md-h md-h${b.level}`}>
            {inline(b.text)}
          </p>
        );
      case 'quote':
        return <blockquote key={i}>{withBreaks(b.lines)}</blockquote>;
      case 'ul':
        return (
          <ul key={i}>
            {b.items.map((it, j) => (
              <li key={j}>{inline(it)}</li>
            ))}
          </ul>
        );
      case 'ol':
        return (
          <ol key={i}>
            {b.items.map((it, j) => (
              <li key={j}>{inline(it)}</li>
            ))}
          </ol>
        );
      case 'p':
        return <p key={i}>{withBreaks(b.lines)}</p>;
    }
  });
}

function withBreaks(lines: string[]): React.ReactNode[] {
  return lines.flatMap((l, i) => (i === 0 ? [inline(l, i)] : [<br key={`br${i}`} />, inline(l, i)]));
}

// `code` | **bold** | __bold__ | *italic* | [text](https://url)
const INLINE = /(`[^`]+`)|(\*\*[^*]+?\*\*|__[^_]+?__)|(\*[^*\s](?:[^*]*?[^*\s])?\*)|(\[[^\]]+\]\(https?:\/\/[^)\s]+\))/g;

function inline(text: string, key: number | string = 0): React.ReactNode {
  const out: React.ReactNode[] = [];
  let last = 0;
  let n = 0;
  for (const m of text.matchAll(INLINE)) {
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    const tok = m[0];
    if (m[1]) out.push(<code key={n++}>{tok.slice(1, -1)}</code>);
    else if (m[2]) out.push(<strong key={n++}>{inline(tok.slice(2, -2), n)}</strong>);
    else if (m[3]) out.push(<em key={n++}>{inline(tok.slice(1, -1), n)}</em>);
    else if (m[4]) {
      const [, label, url] = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(tok) ?? [];
      out.push(
        <a
          key={n++}
          href={url}
          onClick={(e) => {
            e.preventDefault();
            openUrl(url).catch(() => {});
          }}
        >
          {label}
        </a>
      );
    }
    last = at + tok.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <React.Fragment key={key}>{out}</React.Fragment>;
}
