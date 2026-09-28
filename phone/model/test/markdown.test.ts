// Markdown: the phone's tree against the page VS Code renders with its real files (marked,
// DOMPurify and extension/media/markdown.js in jsdom).
import { describe, expect, it } from 'vitest';
import { knownEntities, opensExternally, parse, plainText, safeHref } from '../src/markdown.ts';
import type { Block, Inline, InlineRun } from '../src/markdown.ts';
import { describeMarkdown } from './helpers/describe.ts';
import { fixtures } from './helpers/fixtures.ts';
import { squash } from './helpers/lines.ts';
import { UNSAFE, WRITTEN } from './helpers/markdown-corpus.ts';
import { page, renderMarkdown } from './helpers/vscode.ts';

const HOME = '/Users/fixture';

/** Every reply an agent gave in the recordings. */
const recorded = [...new Set(fixtures.flatMap(f => f.events.filter(e => e.kind === 'output').map(e => e.payload as { role?: string; text?: string }).filter(p => (p.role ?? 'assistant') === 'assistant' || p.role === 'reasoning' || p.role === 'plan').map(p => String(p.text ?? ''))))];

function structure(source: string): { mine: string[]; theirs: string[] } {
  return { mine: describeMarkdown(parse(source, { home: HOME })), theirs: renderMarkdown(source, HOME).lines };
}

function everyInline(blocks: ReadonlyArray<Block | InlineRun>, each: (n: Inline) => void): void {
  const walk = (nodes: ReadonlyArray<Inline>): void => { for (const n of nodes) { each(n); if ('children' in n) walk(n.children); } };
  for (const b of blocks) {
    if (b.type === 'paragraph' || b.type === 'heading' || b.type === 'inline') walk(b.children);
    else if (b.type === 'quote') everyInline(b.children, each);
    else if (b.type === 'list') for (const item of b.items) everyInline(item.children, each);
    else if (b.type === 'table') { for (const cell of b.head) walk(cell); for (const row of b.rows) for (const cell of row) walk(cell); }
  }
}

describe('Markdown parity with VS Code', () => {
  it('the replies of the recordings have the same structure', () => {
    let lines = 0;
    const different: string[] = [];
    for (const source of recorded) {
      const { mine, theirs } = structure(source);
      lines += theirs.length;
      if (JSON.stringify(mine) !== JSON.stringify(theirs)) different.push(`${JSON.stringify(source.slice(0, 80))}\n  phone:   ${JSON.stringify(mine)}\n  VS Code: ${JSON.stringify(theirs)}`);
    }
    console.log(`Markdown, replies of the recordings: ${recorded.length} replies, ${lines} lines compared, ${different.length} different`);
    expect(different).toEqual([]);
  });

  it('written Markdown has the same structure, construct by construct', () => {
    let lines = 0;
    const different: string[] = [];
    for (const source of WRITTEN) {
      const { mine, theirs } = structure(source);
      lines += theirs.length;
      if (JSON.stringify(mine) !== JSON.stringify(theirs)) different.push(`${JSON.stringify(source)}\n  phone:   ${JSON.stringify(mine)}\n  VS Code: ${JSON.stringify(theirs)}`);
    }
    console.log(`Markdown, written inputs: ${WRITTEN.length} inputs, ${lines} lines compared, ${different.length} different`);
    expect(different).toEqual([]);
  });

  it('unsafe input shows the same words, and never becomes markup or an address that runs code', () => {
    const different: string[] = [];
    for (const source of UNSAFE) {
      const tree = parse(source, { home: HOME });
      const mine = squash(plainText(tree));
      const theirs = renderMarkdown(source, HOME).words;
      if (mine !== theirs) different.push(`${JSON.stringify(source)}\n  phone:   ${JSON.stringify(mine)}\n  VS Code: ${JSON.stringify(theirs)}`);
      everyInline(tree, n => {
        if (n.type !== 'link') return;
        expect(n.href === null || !/^\s*(?:javascript|vbscript|data|file):/i.test(n.href.replace(/[\u0000- ]/g, '')), `${source} keeps ${String(n.href)}`).toBe(true);
        expect(n.opens).toBe(n.href !== null && /^https?:\/\//i.test(n.href));
      });
    }
    console.log(`Markdown, unsafe inputs: ${UNSAFE.length} inputs, ${different.length} with other words than VS Code`);
    expect(different).toEqual([]);
  });

  it('keeps an address only when the sanitizer keeps it', () => {
    const p = page();
    const purify = (p.window as unknown as { DOMPurify: { sanitize(html: string, config: unknown): string } }).DOMPurify;
    const addresses = ['https://example.com', 'http://example.com/a?b=c#d', 'HTTPS://EXAMPLE.COM', 'ftp://files.example/x', 'mailto:a@b.co', 'tel:+15551234', 'sms:+1555', 'javascript:alert(1)', 'JAVASCRIPT:alert(1)', ' javascript:alert(1)', 'java\nscript:alert(1)',
      'vbscript:x', 'data:text/html,hi', 'file:///etc/passwd', '/relative/path', './here', '../up', '#anchor', '?query', 'docs/readme.md', 'example.com/path', 'custom-scheme:thing', 'a:b', '//protocol.relative/x', 'x-apple:settings', ''];
    for (const address of addresses) {
      const el = p.document.createElement('div');
      el.innerHTML = purify.sanitize(`<a href="${address.replace(/&/g, '&amp;').replace(/"/g, '&quot;')}">x</a>`, { ALLOWED_TAGS: ['a'], ALLOWED_ATTR: ['href'] });
      const kept = el.querySelector('a')?.hasAttribute('href') ?? false;
      expect(safeHref(address) !== null, `${JSON.stringify(address)}: the sanitizer ${kept ? 'keeps' : 'removes'} it`).toBe(kept);
    }
    expect(opensExternally('https://example.com')).toBe(true);
    expect(opensExternally('mailto:a@b.co')).toBe(false);
    expect(opensExternally(null)).toBe(false);
  });

  it('reads the entities it knows as a browser reads them', () => {
    const p = page();
    const el = p.document.createElement('div');
    for (const [name, char] of knownEntities()) {
      el.innerHTML = `&${name};`;
      expect(char, name).toBe(el.textContent);
    }
    for (const code of [0, 9, 38, 128, 130, 150, 159, 160, 8212, 0xd800, 0x10ffff, 0x110000, 128512]) {
      el.innerHTML = `&#${code};`;
      const tree = parse(`x&#${code};y`);
      expect(plainText(tree), `&#${code};`).toBe(`x${el.textContent}y`);
    }
    expect(knownEntities().size).toBeGreaterThan(300);
  });

  it('gives the words of a tree', () => {
    expect(plainText(parse('# Title\n\nSome **bold** text.\n\n- a\n- b\n\n| x | y |\n|---|---|\n| 1 | 2 |'))).toBe('Title\nSome bold text.\na\nb\nx | y\n1 | 2');
  });
});
