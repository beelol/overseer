// VS Code's side of the comparison: the extension's real files, loaded into a page.
//
// Nothing of the extension is copied or rewritten here. The files are read from extension/media
// and run in jsdom in the order the webview loads them (extension/src/webview-html.js): ui.js,
// logos.js, marked, DOMPurify, highlight.js, markdown.js, conversation.js. The page is then read
// the way a person reads it:
//
//   a row's text          the text of its elements (textContent), white space squashed
//   an icon               the codicon's name, from its class (codicon-check is "check")
//   hidden things         an element with the `hidden` attribute is not read
//   tooltips and labels   the `title` and `aria-label` attributes, where VS Code keeps the full
//                         path, the full command or the name of a status
//   a tool call's detail  the row is opened (as a click does) and its two sections are read
//   depth                 0 for what a turn holds; one more inside a fold of steps, under a tool
//                         call (its children) and inside a child's block
import fs from 'node:fs';
import path from 'node:path';
import { JSDOM } from 'jsdom';
import { repoRoot } from './fixtures.ts';
import { mark, squash } from './lines.ts';
import type { Description, Line } from './lines.ts';
import type { DaemonEvent, Run } from '../../src/types.ts';

const media = path.join(repoRoot, 'extension/media');
const FILES = ['ui.js', 'logos.js', 'vendor/marked.umd.js', 'vendor/purify.min.js', 'vendor/highlight.min.js', 'markdown.js', 'conversation.js'];
const sources = FILES.map(f => fs.readFileSync(path.join(media, f), 'utf8'));

export interface Page {
  window: JSDOM['window'];
  document: Document;
}

/** A page with the chat's scripts loaded, as a webview has them. */
export function page(home = ''): Page {
  const dom = new JSDOM('<!doctype html><html lang="en"><body></body></html>', { runScripts: 'outside-only', pretendToBeVisual: true });
  (dom.window as unknown as { __overseerHome: string }).__overseerHome = home;
  for (const source of sources) dom.window.eval(source);
  return { window: dom.window, document: dom.window.document };
}

interface RealConversation {
  setRun(msg: unknown): void;
  add(event: DaemonEvent): void;
  truncated(text: string): void;
}

export interface VsCodeChat {
  root: HTMLElement;
  setRun(run: Run, children: ReadonlyArray<Run>): void;
  add(event: DaemonEvent): void;
  read(): Description;
}

/** The real conversation view on a page of its own. */
export function chat(home = ''): VsCodeChat {
  const p = page(home);
  const root = p.document.createElement('div');
  p.document.body.append(root);
  const Conversation = (p.window as unknown as { OverseerConversation: new (root: HTMLElement, opts: unknown) => RealConversation }).OverseerConversation;
  const posted: unknown[] = [];
  const conversation = new Conversation(root, { post: (m: unknown) => posted.push(m) });
  return {
    root,
    // What extension/src/run-feed.js `runMessage` sends for a run: the run and its descendants.
    setRun: (run, children) => conversation.setRun(JSON.parse(JSON.stringify({ run, children: children.map(c => ({ id: c.id, title: c.title, status: c.status, parent: c.parent_run_id, evidence: c.relation_source })) }))),
    add: event => conversation.add(JSON.parse(JSON.stringify(event)) as DaemonEvent),
    read: () => readConversation(p, root),
  };
}

const shown = (el: Element | null | undefined): el is HTMLElement => !!el && !(el as HTMLElement).hidden;
const has = (el: Element, name: string): boolean => el.classList.contains(name);
const kids = (el: Element): Element[] => [...el.children];
const text = (el: Element | null | undefined): string => squash(el?.textContent);
const attr = (el: Element | null | undefined, name: string): string | null => el?.getAttribute(name) ?? null;

/** The name of the codicon in or on an element: "check" for codicon-check. */
function icon(el: Element | null | undefined): string | null {
  const i = el?.matches('.codicon') ? el : el?.querySelector('.codicon');
  const name = [...(i?.classList ?? [])].find(c => c.startsWith('codicon-'));
  return name ? name.slice('codicon-'.length) : null;
}

export function readConversation(p: Page, root: HTMLElement): Description {
  const banner = root.querySelector(':scope > .conv-banner');
  const working = root.querySelector(':scope > .working');
  const lines: Line[] = [];
  for (const turn of root.querySelectorAll(':scope > .conv-turns > section.turn')) {
    const n = (turn as HTMLElement).dataset['turn'] ?? '';
    const user = turn.querySelector(':scope > .msg.user');
    if (shown(user)) lines.push({ depth: 0, turn: n, kind: 'user', label: attr(user, 'aria-label'), text: user.querySelector('.text')?.textContent ?? '' });
    const body = turn.querySelector(':scope > .turn-body');
    if (body) walk(p, body, 0, n, lines);
    const foot = turn.querySelector(':scope > .turn-foot');
    if (shown(foot)) {
      const done = foot.querySelector('.done') as HTMLElement, usage = foot.querySelector('.usage') as HTMLElement;
      lines.push({
        depth: 0, turn: n, kind: 'footer', state: [...done.classList].find(c => c !== 'done') ?? null, icon: icon(done), text: text(done), tooltip: attr(done, 'title'),
        duration: text(foot.querySelector('.dur')), usage: text(usage), usageDetail: attr(usage, 'title') ?? '',
      });
    }
  }
  return { banner: shown(banner) ? text(banner) : null, working: shown(working) ? text(working.querySelector('.working-label')) : null, lines };
}

function walk(p: Page, container: Element, depth: number, turn: string, out: Line[]): void {
  for (const el of kids(container)) {
    if (!shown(el)) continue;
    if (has(el, 'tool-children')) continue; // read with the tool call it belongs to
    if (has(el, 'msg')) {
      const body = el.querySelector(':scope > .text') as HTMLElement;
      const agent = has(el, 'agent');
      out.push({ depth, turn, kind: 'message', label: attr(el, 'aria-label'), ...(agent ? { markdown: readMarkdown(body) } : { text: body.textContent ?? '' }) });
    } else if (has(el, 'thinking')) {
      out.push({ depth, turn, kind: 'thinking', label: text(el.querySelector('summary')), icon: icon(el.querySelector('summary')), markdown: readMarkdown(el.querySelector(':scope > .text') as HTMLElement) });
    } else if (has(el, 'steps')) {
      const fold = el.querySelector(':scope > details.steps-fold');
      const list = (fold ?? el).querySelector(':scope > .steps-list') as HTMLElement;
      if (fold) {
        const sum = fold.querySelector(':scope > summary') as HTMLElement;
        out.push({ depth, turn, kind: 'steps', icon: icon(sum), label: text(sum.querySelector('.steps-label')), summary: text(sum.querySelector('.steps-verbs')), failed: text(sum.querySelector('.steps-failed')), tooltip: attr(sum, 'title') });
      }
      for (const item of kids(list)) if (item.matches('details.tool')) tool(p, item as HTMLDetailsElement, depth + (fold ? 1 : 0), turn, out);
      const edits = el.querySelector(':scope > .steps-edits');
      if (edits) walk(p, edits, depth, turn, out);
    } else if (el.matches('details.tool')) {
      tool(p, el as HTMLDetailsElement, depth, turn, out);
    } else if (has(el, 'edit')) {
      out.push({ depth, turn, kind: 'edit', icon: icon(el), files: [...el.querySelectorAll('button.edit-path')].map(b => ({ name: b.textContent ?? '', tooltip: attr(b, 'title') })) });
    } else if (has(el, 'perm-card')) {
      const head = el.querySelector(':scope > .perm-head') as HTMLElement;
      out.push({
        depth, turn, kind: 'permission', state: [...el.classList].find(c => c !== 'perm-card') ?? null, icon: icon(head), text: text(head), full: attr(head, 'title'),
        preview: el.querySelector(':scope > pre.perm-preview')?.textContent ?? null,
        actions: [...el.querySelectorAll(':scope > .perm-actions > button')].map(b => ({ label: text(b), allow: (b as HTMLElement).dataset['permission'] === 'allow' })),
        requestLabel: text(el.querySelector(':scope > details.perm-input > summary')), requestHint: attr(el.querySelector(':scope > details.perm-input > summary'), 'title'),
        request: el.querySelector(':scope > details.perm-input > pre')?.textContent ?? null,
      });
    } else if (has(el, 'error-block')) {
      const button = el.querySelector(':scope > button.sign-in-again');
      out.push({ depth, turn, kind: 'error', icon: icon(el.querySelector('.error-head')), class: attr(el, 'data-class') || attr(el, 'title'), title: text(el.querySelector('.error-head strong')), message: el.querySelector(':scope > .text')?.textContent ?? '', signIn: button ? { label: text(button), says: attr(button, 'aria-label') } : null });
    } else if (el.matches('details.child')) {
      const sum = el.querySelector(':scope > summary') as HTMLElement;
      out.push({ depth, turn, kind: 'child', icon: icon(sum), run: (el as HTMLElement).dataset['run'], title: text(sum.querySelector('.child-title')), status: attr(sum.querySelector('.status'), 'aria-label'), tooltip: attr(sum, 'title') });
      walk(p, el.querySelector(':scope > .child-body') as HTMLElement, depth + 1, turn, out);
    } else if (has(el, 'sys')) {
      out.push({ depth, turn, kind: 'note', text: text(el), status: [...el.classList].find(c => c.startsWith('status-') && c !== 'status-line')?.slice('status-'.length) ?? null, icon: icon(el), tooltip: attr(el, 'title') });
    } else {
      throw new Error(`the page has something this does not read: <${el.tagName.toLowerCase()} class="${el.className}">`);
    }
  }
}

function tool(p: Page, el: HTMLDetailsElement, depth: number, turn: string, out: Line[]): void {
  const result = el.querySelector(':scope > summary > .tool-result') as HTMLElement;
  const state = has(result, 'bad') ? `failed: ${text(result)}` : has(result, 'run') ? 'running' : result.querySelector('.add') ? `${text(result.querySelector('.add'))} ${text(result.querySelector('.del'))}` : icon(result) === 'check' ? 'ok' : `unread: ${result.outerHTML}`;
  // Open it, as a click does: VS Code fills the two sections when the row opens.
  const was = el.open;
  el.open = true;
  el.dispatchEvent(new p.window.Event('toggle'));
  const [given, returned] = [...el.querySelectorAll(':scope > .tool-section')];
  const section = (s: Element | undefined): { label: string; text: string } | null => (s?.querySelector('pre') ? { label: text(s.querySelector('.label')), text: s.querySelector('pre')?.textContent ?? '' } : null);
  const summary = el.querySelector(':scope > summary > .tool-summary') as HTMLElement;
  out.push({
    depth, turn, kind: 'tool', id: el.dataset['tool'] ?? '', name: el.dataset['name'] ?? '', icon: icon(el.querySelector(':scope > summary > .tool-icon')), verb: el.querySelector(':scope > summary > .tool-verb')?.textContent ?? '',
    target: summary.textContent ?? '', code: has(summary, 'mono'), full: attr(el, 'title') ?? '', result: state, input: section(given), output: section(returned), outputIsError: !!returned?.querySelector('pre.error'),
    note: returned?.querySelector('.muted') ? text(returned.querySelector('.muted')) : null,
  });
  el.open = was;
  const under = el.nextElementSibling;
  if (under && has(under, 'tool-children')) walk(p, under, depth + 1, turn, out);
}

// ------------------------------------------------------------------ Markdown, as rendered

const INLINE = new Set(['STRONG', 'EM', 'DEL', 'S', 'CODE', 'A', 'BR', 'BUTTON', 'SPAN', 'INPUT', 'B', 'I']);

function inline(node: Node): string {
  if (node.nodeType === 3) return mark.text(node.textContent ?? '');
  if (node.nodeType !== 1) return '';
  const el = node as Element;
  const inner = (): string => [...el.childNodes].map(inline).join('');
  switch (el.tagName) {
    case 'STRONG': return `<b>${inner()}</b>`;
    case 'EM': return `<i>${inner()}</i>`;
    case 'DEL': case 'S': return `<s>${inner()}</s>`;
    case 'CODE': return `<code>${inner()}</code>`;
    case 'BR': return '<br>';
    case 'INPUT': return '';
    case 'A': return `<a href=${JSON.stringify(attr(el, 'href'))} tooltip=${JSON.stringify(attr(el, 'title'))}>${inner()}</a>`;
    case 'BUTTON': return has(el, 'md-long') ? `<long full=${JSON.stringify((attr(el, 'title') ?? '').replace(/\nClick to copy$/, ''))}>${mark.text(el.textContent ?? '')}</long>` : '';
    default: return inner();
  }
}

/** A rendered reply as lines: a block on each, what it holds indented under it. */
export function readMarkdown(root: Element, indent = ''): string[] {
  const out: string[] = [];
  let run: Node[] = [];
  const flush = (as: string): void => {
    const words = squash(run.map(inline).join(''));
    if (words) out.push(`${indent}${as}: ${words}`);
    run = [];
  };
  const loose = root.tagName === 'LI' ? 'text' : 'p';
  for (const node of root.childNodes) {
    if (node.nodeType === 3 || (node.nodeType === 1 && INLINE.has((node as Element).tagName))) { run.push(node); continue; }
    if (node.nodeType !== 1) continue;
    flush(loose);
    const el = node as Element;
    const tag = el.tagName;
    if (tag === 'P') { const words = squash([...el.childNodes].map(inline).join('')); if (words) out.push(`${indent}p: ${words}`); }
    else if (/^H[1-6]$/.test(tag)) out.push(`${indent}h${tag[1]}: ${squash([...el.childNodes].map(inline).join(''))}`);
    else if (tag === 'HR') out.push(`${indent}rule`);
    else if (tag === 'BLOCKQUOTE') { out.push(`${indent}quote`); out.push(...readMarkdown(el, indent + '  ')); }
    else if (tag === 'UL' || tag === 'OL') {
      out.push(`${indent}${tag === 'OL' ? `ol start=${attr(el, 'start') ?? '1'}` : 'ul'}`);
      for (const li of kids(el)) {
        if (li.tagName !== 'LI') continue;
        const box = li.querySelector(':scope > input, :scope > p:first-child > input');
        out.push(`${indent}  li${box ? ((box as HTMLInputElement).checked ? ' [x]' : ' [ ]') : ''}`);
        out.push(...readMarkdown(li, indent + '    '));
      }
    } else if (tag === 'DIV' && has(el, 'codeblock')) {
      const code = el.querySelector('pre code') ?? el.querySelector('pre');
      const body = code?.textContent ?? '';
      out.push(`${indent}code ${JSON.stringify({ label: text(el.querySelector('.codeblock-lang')), lines: body.split('\n').length, collapsed: has(el, 'collapsed'), more: text(el.querySelector('.codeblock-more')) || null, text: body.replace(/\n$/, '') })}`);
    } else if (tag === 'DIV' && has(el, 'md-table')) {
      const table = el.querySelector('table') as HTMLTableElement;
      const cells = (row: Element): string => kids(row).map(c => squash([...c.childNodes].map(inline).join(''))).join(' | ');
      const head = table.querySelector('thead tr');
      out.push(`${indent}table align=${JSON.stringify(head ? kids(head).map(c => attr(c, 'align')) : [])}`);
      if (head) out.push(`${indent}  head: ${cells(head)}`);
      for (const row of table.querySelectorAll('tbody tr')) out.push(`${indent}  row: ${cells(row)}`);
    } else out.push(...readMarkdown(el, indent));
  }
  flush(loose);
  return out;
}

/** A reply rendered by the extension's markdown.js, on a page of its own. */
export function renderMarkdown(source: string, home = ''): { lines: string[]; words: string; element: HTMLElement } {
  const p = page(home);
  const target = p.document.createElement('div');
  p.document.body.append(target);
  (p.window as unknown as { OverseerMarkdown: { render(target: HTMLElement, text: string, opts: unknown): void } }).OverseerMarkdown.render(target, source, { post: () => {} });
  for (const head of target.querySelectorAll('.codeblock-head, .codeblock-more')) head.setAttribute('data-chrome', '');
  const words = squash([...target.childNodes].map(n => wordsOf(n)).join(' '));
  return { lines: readMarkdown(target), words, element: target };
}

/** The words of a rendered reply, without the heads of code blocks (the language and the buttons). */
function wordsOf(node: Node): string {
  if (node.nodeType === 3) return node.textContent ?? '';
  if (node.nodeType !== 1) return '';
  const el = node as Element;
  if (el.hasAttribute('data-chrome') || el.tagName === 'INPUT') return '';
  const block = /^(P|DIV|LI|UL|OL|TR|TD|TH|H[1-6]|BLOCKQUOTE|PRE|TABLE|THEAD|TBODY|HR|BR)$/.test(el.tagName);
  const inner = [...el.childNodes].map(wordsOf).join('');
  return block ? ` ${inner} ` : inner;
}
