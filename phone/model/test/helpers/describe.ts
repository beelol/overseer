// The phone's side of the comparison: the model's rows as the same plain description the page is
// read into (test/helpers/vscode.ts).
import { permissionActions, requestText, rowsOf, toolDetail, markdownOf } from '../../src/conversation.ts';
import type { Conversation, Row } from '../../src/conversation.ts';
import type { Block, Inline, InlineRun } from '../../src/markdown.ts';
import { TEXT } from '../../src/text.ts';
import { mark, squash } from './lines.ts';
import type { Description, Line } from './lines.ts';

export function describeConversation(c: Conversation): Description {
  return { banner: c.banner, working: c.working.shown ? c.working.label : null, lines: rowsOf(c).map(row => line(row, c)) };
}

function line(row: Row, c: Conversation): Line {
  const at = { depth: row.depth, turn: String(row.turnNumber) };
  switch (row.kind) {
    case 'user': return { ...at, kind: 'user', label: row.label, text: row.text };
    case 'message': return { ...at, kind: 'message', label: row.label, ...(row.markdown ? { markdown: describeMarkdown(markdownOf(row, c)) } : { text: row.text }) };
    case 'thinking': return { ...at, kind: 'thinking', label: row.label, icon: row.icon, markdown: describeMarkdown(markdownOf(row, c)) };
    case 'steps': return { ...at, kind: 'steps', icon: row.icon, label: row.label, summary: row.summary, failed: row.failedText, tooltip: row.tooltip };
    case 'tool': {
      const d = toolDetail(row);
      const r = row.result;
      return {
        ...at, kind: 'tool', id: row.id, name: row.name, icon: row.icon, verb: row.verb, target: row.target, code: row.code, full: row.full,
        result: r.state === 'failed' ? `failed: ${r.text}` : r.state === 'changed' ? `${r.addedText} ${r.removedText}` : r.state,
        input: d.input, output: d.output ? { label: d.output.label, text: d.output.text } : null, outputIsError: !!d.output?.error, note: d.note,
      };
    }
    case 'edit': return { ...at, kind: 'edit', icon: row.icon, files: row.files.map(f => ({ name: f.name, tooltip: f.tooltip })) };
    case 'permission': return {
      ...at, kind: 'permission', state: row.state, icon: row.icon, text: row.text, full: row.full, preview: row.preview, actions: permissionActions(row).map(a => ({ label: a.label, allow: a.allow })),
      requestLabel: TEXT.conversation.request, requestHint: TEXT.conversation.requestHint, request: requestText(row),
    };
    case 'error': return { ...at, kind: 'error', icon: row.icon, class: row.class, title: row.title, message: row.message, signIn: row.signIn ? { label: TEXT.conversation.signInAgain, says: TEXT.conversation.signInAgainLabel } : null };
    case 'child': return { ...at, kind: 'child', icon: row.icon, run: row.childRun, title: row.title, usage: row.usage, status: row.statusText, tooltip: row.tooltip };
    case 'note': return { ...at, kind: 'note', text: row.text, link: row.link?.label ?? null, detail: row.detail ?? null, status: row.status, icon: row.icon, tooltip: row.tooltip };
    case 'footer': return { ...at, kind: 'footer', state: row.state, icon: row.icon, text: row.text, tooltip: row.tooltip, duration: row.duration, usage: row.usage, usageDetail: row.usageDetail };
  }
}

function inline(nodes: ReadonlyArray<Inline>): string {
  return nodes.map((n): string => {
    switch (n.type) {
      case 'text': return mark.text(n.text);
      case 'strong': return `<b>${inline(n.children)}</b>`;
      case 'emphasis': return `<i>${inline(n.children)}</i>`;
      case 'strike': return `<s>${inline(n.children)}</s>`;
      case 'code': return `<code>${inline(n.parts)}</code>`;
      case 'break': return '<br>';
      // VS Code's tooltip on a link is its address; the title written in the Markdown shows only where the address was removed.
      case 'link': return `<a href=${JSON.stringify(n.href)} tooltip=${JSON.stringify(n.href ?? n.title)}>${inline(n.children)}</a>`;
      case 'long': return `<long full=${JSON.stringify(n.full)}>${mark.text(n.text)}</long>`;
    }
  }).join('');
}

/** A tree as lines, in the form the rendered page is read into. */
export function describeMarkdown(blocks: ReadonlyArray<Block | InlineRun>, indent = ''): string[] {
  const out: string[] = [];
  for (const b of blocks) {
    switch (b.type) {
      case 'paragraph': { const words = squash(inline(b.children)); if (words) out.push(`${indent}p: ${words}`); break; }
      case 'inline': { const words = squash(inline(b.children)); if (words) out.push(`${indent}text: ${words}`); break; }
      case 'heading': out.push(`${indent}h${b.level}: ${squash(inline(b.children))}`); break;
      case 'rule': out.push(`${indent}rule`); break;
      case 'quote': out.push(`${indent}quote`); out.push(...describeMarkdown(b.children, indent + '  ')); break;
      case 'list':
        out.push(`${indent}${b.ordered ? `ol start=${b.start}` : 'ul'}`);
        for (const item of b.items) {
          out.push(`${indent}  li${item.checked === null ? '' : item.checked ? ' [x]' : ' [ ]'}`);
          out.push(...describeMarkdown(item.children, indent + '    '));
        }
        break;
      case 'code':
        out.push(`${indent}code ${JSON.stringify({ label: b.label, lines: b.lines, collapsed: b.collapsed, more: b.collapsed ? `Show all ${b.lines} lines` : null, text: b.text })}`);
        break;
      case 'table':
        out.push(`${indent}table align=${JSON.stringify(b.align)}`);
        out.push(`${indent}  head: ${b.head.map(cell => squash(inline(cell))).join(' | ')}`);
        for (const row of b.rows) out.push(`${indent}  row: ${row.map(cell => squash(inline(cell))).join(' | ')}`);
        break;
    }
  }
  return out;
}
