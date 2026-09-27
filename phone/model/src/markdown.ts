// The agent's replies as a small tree, so the app can show them with its own views.
//
// VS Code renders replies with marked (GitHub's flavour, single line ends do not break), cleans
// the result with DOMPurify and shortens long unbroken tokens (extension/media/markdown.js). This
// reads the same Markdown into the same structure: paragraphs, headings, lists (with tasks),
// tables, code blocks with their language, quotes, rules; and inside them text, strong, emphasis,
// strike-through, inline code, links, line breaks and shortened long tokens.
//
// What a line of Markdown means is decided by marked's own rules (src/marked-lexer.ts). What
// becomes of the result is decided here: where VS Code builds a page and cleans it, this builds
// the tree a cleaned page would show.
//
// Raw HTML never becomes a view. A tag is dropped and its text kept, as the sanitizer does for a
// tag it does not allow; what a script, style or similar element holds is dropped with it. A link
// keeps its address only when the sanitizer would keep it, and opens only when VS Code would
// open it (http and https).
//
// test/markdown.test.ts renders the same inputs with the extension's real files and compares.

import { lex } from './marked-lexer.ts';
import type { Token } from './marked-lexer.ts';
import { shortPath } from './text.ts';

export interface Text { readonly type: 'text'; readonly text: string }
export interface Strong { readonly type: 'strong'; readonly children: ReadonlyArray<Inline> }
export interface Emphasis { readonly type: 'emphasis'; readonly children: ReadonlyArray<Inline> }
export interface Strike { readonly type: 'strike'; readonly children: ReadonlyArray<Inline> }
export interface InlineCode { readonly type: 'code'; readonly text: string; readonly parts: ReadonlyArray<Text | Long> }
/** `title` is the title written in the Markdown. VS Code does not show it: its tooltip is the address. */
export interface Link { readonly type: 'link'; readonly href: string | null; readonly title: string | null; readonly opens: boolean; readonly children: ReadonlyArray<Inline> }
export interface LineBreak { readonly type: 'break' }
/** An unbroken token of 60 characters or more, shown short; `full` is the whole of it. */
export interface Long { readonly type: 'long'; readonly text: string; readonly full: string; readonly path: boolean }
export type Inline = Text | Strong | Emphasis | Strike | InlineCode | Link | LineBreak | Long;

export interface Paragraph { readonly type: 'paragraph'; readonly children: ReadonlyArray<Inline> }
export interface Heading { readonly type: 'heading'; readonly level: 1 | 2 | 3 | 4 | 5 | 6; readonly children: ReadonlyArray<Inline> }
/** Text of a tight list item: inline content that is not its own paragraph. */
export interface InlineRun { readonly type: 'inline'; readonly children: ReadonlyArray<Inline> }
export interface ListItem { readonly checked: boolean | null; readonly children: ReadonlyArray<Block | InlineRun> }
export interface List { readonly type: 'list'; readonly ordered: boolean; readonly start: number; readonly items: ReadonlyArray<ListItem> }
export interface Table {
  readonly type: 'table';
  readonly align: ReadonlyArray<'left' | 'right' | 'center' | null>;
  readonly head: ReadonlyArray<ReadonlyArray<Inline>>;
  readonly rows: ReadonlyArray<ReadonlyArray<ReadonlyArray<Inline>>>;
}
export interface CodeBlock {
  readonly type: 'code';
  /** As written after the fence: "ts", "c++". Empty when none was given. */
  readonly language: string;
  /** What the block's head says: the language, or "text". */
  readonly label: string;
  /** Without the line end VS Code adds after the last line. */
  readonly text: string;
  /** As VS Code counts them for "Show all N lines": one more than the text has. */
  readonly lines: number;
  /** More than 24 lines: shown cut, with a way to show all. */
  readonly collapsed: boolean;
}
export interface Quote { readonly type: 'quote'; readonly children: ReadonlyArray<Block> }
export interface Rule { readonly type: 'rule' }
export type Block = Paragraph | Heading | List | Table | CodeBlock | Quote | Rule;

export interface MarkdownOptions {
  /** The Mac's home folder, when known: a long path under it starts with "~". */
  readonly home?: string;
}

// ------------------------------------------------------------------ addresses

/** What DOMPurify keeps as an address: a known scheme, or no scheme at all. */
const SAFE_ADDRESS = /^(?:(?:(?:f|ht)tps?|mailto|tel|callto|sms|cid|xmpp|matrix):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i;
/** What it removes before it looks: white space and control characters. */
const ADDRESS_NOISE = /[\u0000-\u0020\u00A0\u1680\u180E\u2000-\u2029\u205F\u3000]/g;

/** The address when VS Code's sanitizer keeps it, else `null`. */
export function safeHref(href: string): string | null {
  // An empty address is kept: it names the page itself.
  return !href || SAFE_ADDRESS.test(String(href).replace(ADDRESS_NOISE, '')) ? href : null;
}

/** True for http and https: the only addresses VS Code opens from a reply. */
export function opensExternally(href: string | null): boolean {
  return href !== null && /^https?:\/\//i.test(href);
}

/** marked's `cleanUrl`: the address with what an address cannot hold written as %XX. */
function cleanUrl(href: string): string | null {
  try {
    return encodeURI(href).replace(/%25/g, '%');
  } catch {
    return null;
  }
}

// ------------------------------------------------------------------ entities

const ENTITY_TABLE = 'nbsp:a0 iexcl:a1 cent:a2 pound:a3 curren:a4 yen:a5 brvbar:a6 sect:a7 uml:a8 copy:a9 ordf:aa laquo:ab not:ac shy:ad reg:ae macr:af deg:b0 plusmn:b1 sup2:b2 sup3:b3 acute:b4 micro:b5 para:b6 middot:b7 cedil:b8 sup1:b9 ordm:ba raquo:bb frac14:bc frac12:bd frac34:be iquest:bf Agrave:c0 Aacute:c1 Acirc:c2 Atilde:c3 Auml:c4 Aring:c5 AElig:c6 Ccedil:c7 Egrave:c8 Eacute:c9 Ecirc:ca Euml:cb Igrave:cc Iacute:cd Icirc:ce Iuml:cf ETH:d0 Ntilde:d1 Ograve:d2 Oacute:d3 Ocirc:d4 Otilde:d5 Ouml:d6 times:d7 Oslash:d8 Ugrave:d9 Uacute:da Ucirc:db Uuml:dc Yacute:dd THORN:de szlig:df agrave:e0 aacute:e1 acirc:e2 atilde:e3 auml:e4 aring:e5 aelig:e6 ccedil:e7 egrave:e8 eacute:e9 ecirc:ea euml:eb igrave:ec iacute:ed icirc:ee iuml:ef eth:f0 ntilde:f1 ograve:f2 oacute:f3 ocirc:f4 otilde:f5 ouml:f6 divide:f7 oslash:f8 ugrave:f9 uacute:fa ucirc:fb uuml:fc yacute:fd thorn:fe yuml:ff quot:22 amp:26 lt:3c gt:3e apos:27 OElig:152 oelig:153 Scaron:160 scaron:161 Yuml:178 circ:2c6 tilde:2dc ensp:2002 emsp:2003 thinsp:2009 zwnj:200c zwj:200d lrm:200e rlm:200f ndash:2013 mdash:2014 lsquo:2018 rsquo:2019 sbquo:201a ldquo:201c rdquo:201d bdquo:201e dagger:2020 Dagger:2021 permil:2030 lsaquo:2039 rsaquo:203a euro:20ac fnof:192 Alpha:391 Beta:392 Gamma:393 Delta:394 Epsilon:395 Zeta:396 Eta:397 Theta:398 Iota:399 Kappa:39a Lambda:39b Mu:39c Nu:39d Xi:39e Omicron:39f Pi:3a0 Rho:3a1 Sigma:3a3 Tau:3a4 Upsilon:3a5 Phi:3a6 Chi:3a7 Psi:3a8 Omega:3a9 alpha:3b1 beta:3b2 gamma:3b3 delta:3b4 epsilon:3b5 zeta:3b6 eta:3b7 theta:3b8 iota:3b9 kappa:3ba lambda:3bb mu:3bc nu:3bd xi:3be omicron:3bf pi:3c0 rho:3c1 sigmaf:3c2 sigma:3c3 tau:3c4 upsilon:3c5 phi:3c6 chi:3c7 psi:3c8 omega:3c9 thetasym:3d1 upsih:3d2 piv:3d6 bull:2022 hellip:2026 prime:2032 Prime:2033 oline:203e frasl:2044 weierp:2118 image:2111 real:211c trade:2122 alefsym:2135 larr:2190 uarr:2191 rarr:2192 darr:2193 harr:2194 crarr:21b5 lArr:21d0 uArr:21d1 rArr:21d2 dArr:21d3 hArr:21d4 forall:2200 part:2202 exist:2203 empty:2205 nabla:2207 isin:2208 notin:2209 ni:220b prod:220f sum:2211 minus:2212 lowast:2217 radic:221a prop:221d infin:221e ang:2220 and:2227 or:2228 cap:2229 cup:222a int:222b there4:2234 sim:223c cong:2245 asymp:2248 ne:2260 equiv:2261 le:2264 ge:2265 sub:2282 sup:2283 nsub:2284 sube:2286 supe:2287 oplus:2295 otimes:2297 perp:22a5 sdot:22c5 lceil:2308 rceil:2309 lfloor:230a rfloor:230b lang:27e8 rang:27e9 loz:25ca spades:2660 clubs:2663 hearts:2665 diams:2666 Tab:9 NewLine:a excl:21 num:23 dollar:24 percnt:25 lpar:28 rpar:29 ast:2a plus:2b comma:2c period:2e sol:2f colon:3a semi:3b equals:3d quest:3f commat:40 lsqb:5b lbrack:5b bsol:5c rsqb:5d rbrack:5d Hat:5e lowbar:5f grave:60 lcub:7b lbrace:7b verbar:7c vert:7c rcub:7d rbrace:7d check:2713 cross:2717 star:2606 starf:2605 phone:260e half:bd hyphen:2010 dash:2010 nearr:2197 searr:2198 swarr:2199 nwarr:2196 hbar:210f ell:2113 langle:27e8 rangle:27e9 LT:3c GT:3e AMP:26 QUOT:22 COPY:a9 REG:ae';

let named: Map<string, string> | undefined;

/** The names this knows (the ones of HTML 4 and some more); a browser knows about two thousand. */
export function knownEntities(): ReadonlyMap<string, string> {
  if (named === undefined) {
    named = new Map();
    for (const pair of ENTITY_TABLE.split(' ')) {
      const [name = '', code = '0'] = pair.split(':');
      named.set(name, String.fromCodePoint(parseInt(code, 16)));
    }
  }
  return named;
}

/** Windows-1252, which a browser reads the numbers 128 to 159 as. */
const C1 = [0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178];

function numbered(code: number): string {
  if (!code || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) return '\ufffd';
  if (code >= 0x80 && code <= 0x9f) return String.fromCodePoint(C1[code - 0x80] as number);
  return String.fromCodePoint(code);
}

const ENTITY = /^&(?:#(\d{1,7})|#[xX]([0-9a-fA-F]{1,6})|([a-zA-Z][a-zA-Z0-9]*));/;

/** The character an entity at the start of `s` stands for and the entity's length, or undefined. */
function entityAt(s: string): { text: string; length: number } | undefined {
  const m = ENTITY.exec(s);
  if (!m) return undefined;
  if (m[1] !== undefined) return { text: numbered(parseInt(m[1], 10)), length: m[0].length };
  if (m[2] !== undefined) return { text: numbered(parseInt(m[2], 16)), length: m[0].length };
  const known = knownEntities().get(m[3] as string);
  return known === undefined ? undefined : { text: known, length: m[0].length };
}

function decodeEntities(s: string): string {
  if (!s.includes('&')) return s;
  let out = '';
  for (let i = 0; i < s.length;) {
    const e = s[i] === '&' ? entityAt(s.slice(i, i + 40)) : undefined;
    if (e !== undefined) { out += e.text; i += e.length; } else { out += s[i]; i++; }
  }
  return out;
}

// ------------------------------------------------------------------ raw HTML

const COMMENT = /^<!--(?:-?>|[\s\S]*?(?:-->|$))/;
const CLOSE_TAG = /^<\/([a-zA-Z][\w:-]*)\s*>/;
const OPEN_TAG = /^<([a-zA-Z][^\s/>]*)(?:"[^"]*"|'[^']*'|[^'">])*>/;
const OTHER_TAG = /^<[!?][^>]*>/;

/** Elements the sanitizer removes with everything they hold. */
const DROPPED_WITH_CONTENT = new Set(['annotation-xml', 'audio', 'colgroup', 'desc', 'foreignobject', 'head', 'iframe', 'math', 'mi', 'mn', 'mo', 'ms', 'mtext', 'noembed', 'noframes', 'noscript', 'plaintext', 'script', 'style', 'svg', 'template', 'title', 'video', 'xmp']);
/** Tags the sanitizer keeps and a browser starts a new line at: their text does not run into the text before. */
const BREAKS_LINE = /^\/?(?:br|p|li|tr|td|th|h[1-6]|ul|ol|table|thead|tbody|blockquote|pre|hr)$/;

interface Context {
  readonly home: string;
  /** The element whose end is waited for: everything until then is dropped. */
  dropping: string | null;
}

/** A place where a tag was dropped: texts on its two sides are separate texts in VS Code's page. */
interface Cut { readonly cut: true }
type Piece = Inline | Cut;
const CUT: Cut = { cut: true };
const isCut = (p: Piece): p is Cut => 'cut' in p;

const endOf = (name: string): RegExp => new RegExp(`</${name.replace(/[^a-z0-9-]/gi, '')}\\s*>`, 'i');

/** What a raw tag does: a line break for <br>, else nothing but a place where text is cut. */
function rawTag(tag: string, c: Context): Piece {
  const opened = COMMENT.test(tag) ? null : OPEN_TAG.exec(tag);
  if (!opened) return CUT;
  const name = (opened[1] as string).toLowerCase();
  if (name === 'br') return { type: 'break' };
  if (!/\/>$/.test(tag) && DROPPED_WITH_CONTENT.has(name)) c.dropping = name;
  return CUT;
}

/** A block of raw HTML as the text it leaves behind. */
function htmlText(raw: string, c: Context): string {
  let out = '';
  let i = 0;
  while (i < raw.length) {
    if (c.dropping !== null) {
      const end = endOf(c.dropping).exec(raw.slice(i));
      if (!end) return out;
      i += end.index + end[0].length;
      c.dropping = null;
      continue;
    }
    const rest = raw.slice(i);
    if (raw[i] === '<') {
      const tag = COMMENT.exec(rest) || CLOSE_TAG.exec(rest) || OPEN_TAG.exec(rest) || OTHER_TAG.exec(rest);
      if (tag) {
        const name = (/^<\/?([a-zA-Z][^\s/>]*)/.exec(tag[0])?.[1] ?? '').toLowerCase();
        rawTag(tag[0], c);
        if (BREAKS_LINE.test(name) || BREAKS_LINE.test('/' + name)) out += ' ';
        i += tag[0].length;
        continue;
      }
    }
    if (raw[i] === '&') {
      const e = entityAt(rest.slice(0, 40));
      if (e !== undefined) { out += e.text; i += e.length; continue; }
    }
    out += raw[i];
    i++;
  }
  return out;
}

// ------------------------------------------------------------------ from marked's tokens to the tree

/** Neighbouring texts become one, as they are one in the page; not across a dropped tag. */
function merge(nodes: ReadonlyArray<Piece>): Piece[] {
  const out: Piece[] = [];
  for (const n of nodes) {
    const before = out[out.length - 1];
    if (!isCut(n) && n.type === 'text' && before !== undefined && !isCut(before) && before.type === 'text') out[out.length - 1] = { type: 'text', text: before.text + n.text };
    else if (!isCut(n) && n.type === 'text' && !n.text) continue;
    else out.push(n);
  }
  return out;
}

const LONG = /(\S{60,})/;

/** Long unbroken tokens become short ones that keep the whole (markdown.js `shortenLong`). */
function shorten(value: string, home: string): Array<Text | Long> {
  if (!/\S{60,}/.test(value)) return value ? [{ type: 'text', text: value }] : [];
  const out: Array<Text | Long> = [];
  for (const part of value.split(LONG)) {
    if (!part) continue;
    if (!/^\S{60,}$/.test(part)) { out.push({ type: 'text', text: part }); continue; }
    const path = part.includes('/');
    out.push({ type: 'long', text: path ? shortPath(part.replace(/[.,;:)]+$/, ''), home) : part.slice(0, 24) + '…' + part.slice(-12), full: part, path });
  }
  return out;
}

/** The pieces as the tree keeps them: texts merged and shortened, the cuts gone. */
function tidy(nodes: ReadonlyArray<Piece>, c: Context, inLink: boolean): Inline[] {
  const out: Inline[] = [];
  for (const n of merge(nodes)) {
    if (isCut(n)) continue;
    if (n.type === 'text') out.push(...(inLink ? [n] : shorten(n.text, c.home)));
    else if (n.type === 'code') out.push(inLink ? n : { ...n, parts: shorten(n.text, c.home) });
    else out.push(n);
  }
  // Texts that a dropped tag kept apart are one text for whoever shows them.
  const joined: Inline[] = [];
  for (const n of out) {
    const before = joined[joined.length - 1];
    if (n.type === 'text' && before !== undefined && before.type === 'text') joined[joined.length - 1] = { type: 'text', text: before.text + n.text };
    else joined.push(n);
  }
  return joined;
}

function pieces(tokens: ReadonlyArray<Token>, c: Context, inLink: boolean): Piece[] {
  const out: Piece[] = [];
  for (const t of tokens) {
    if (c.dropping !== null) {
      // Inside an element that is dropped with what it holds: only its own end is looked for.
      if (t.type === 'html' && endOf(c.dropping).test(t.raw)) { c.dropping = null; out.push(CUT); }
      continue;
    }
    switch (t.type) {
      case 'text':
        if (t.tokens) out.push(...pieces(t.tokens, c, inLink));
        else out.push({ type: 'text', text: decodeEntities(t.text) });
        break;
      case 'escape': out.push({ type: 'text', text: t.text }); break;
      case 'html': out.push(rawTag(t.raw, c)); break;
      case 'codespan': out.push({ type: 'code', text: t.text, parts: t.text ? [{ type: 'text', text: t.text }] : [] }); break;
      case 'br': out.push({ type: 'break' }); break;
      case 'strong': out.push({ type: 'strong', children: tidy(pieces(t.tokens, c, inLink), c, inLink) }); break;
      case 'em': out.push({ type: 'emphasis', children: tidy(pieces(t.tokens, c, inLink), c, inLink) }); break;
      case 'del': out.push({ type: 'strike', children: tidy(pieces(t.tokens, c, inLink), c, inLink) }); break;
      case 'link': {
        const children = tidy(pieces(t.tokens, c, true), c, true);
        const clean = cleanUrl(t.href);
        // marked writes a link whose address it cannot encode as its text.
        if (clean === null) { out.push(...children); break; }
        const kept = safeHref(decodeEntities(clean));
        out.push({ type: 'link', href: kept, title: t.title ? decodeEntities(t.title) : null, opens: opensExternally(kept), children });
        break;
      }
      case 'image':
        // A picture: the sanitizer removes it. One whose address cannot be encoded is written as its words.
        out.push(cleanUrl(t.href) === null ? { type: 'text', text: decodeEntities(t.text) } : CUT);
        break;
      default: break;
    }
  }
  return out;
}

const inlineOf = (tokens: ReadonlyArray<Token>, c: Context): Inline[] => tidy(pieces(tokens, c, false), c, false);

function codeBlock(language: string | undefined, body: string): CodeBlock {
  const word = /^\S*/.exec(language || '')?.[0] ?? '';
  const label = /^[\w+-]+/.exec(word)?.[0];
  const text = body.replace(/\n$/, '');
  const lines = text.split('\n').length + 1;
  return { type: 'code', language: word, label: label || 'text', text, lines, collapsed: lines > 24 };
}

interface Holder {
  /** Plain text stands as paragraphs: at the top, in a quote, in a loose list. */
  readonly loose: boolean;
  /** Inside a list item, where text that is not a paragraph stands as a run of text. */
  readonly item: boolean;
}

const TOP: Holder = { loose: true, item: false };

function blocksOf(tokens: ReadonlyArray<Token>, c: Context, holder: Holder): Array<Block | InlineRun> {
  const out: Array<Block | InlineRun> = [];
  // Text that stands in no paragraph: a tight item's lines, and what raw HTML leaves behind.
  let run: Piece[] = [];
  let lastWasText = false;
  const flush = (): void => {
    const children = tidy(run, c, false);
    run = [];
    lastWasText = false;
    if (children.some(n => n.type !== 'text' || n.text.trim())) out.push(holder.item ? { type: 'inline', children } : { type: 'paragraph', children });
  };
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i] as Token;
    // What begins inside a dropped element is text of that element, not a block.
    const inside = c.dropping !== null;
    if (t.type === 'space') { lastWasText = false; continue; }
    if (t.type === 'html') {
      const words = htmlText(t.raw, c);
      if (words.trim()) run.push(CUT, { type: 'text', text: (run.length ? ' ' : '') + words.replace(/\s+/g, ' ').trim() + ' ' }, CUT);
      lastWasText = false;
      continue;
    }
    if (t.type === 'text' && !holder.loose) {
      if (lastWasText) run.push({ type: 'text', text: '\n' });
      run.push(...pieces([t], c, false));
      lastWasText = true;
      continue;
    }
    flush();
    switch (t.type) {
      case 'hr': if (!inside) out.push({ type: 'rule' }); break;
      case 'code': if (!inside) out.push(codeBlock(t.lang, t.text)); break;
      case 'heading': {
        const children = inlineOf(t.tokens, c);
        if (!inside) out.push({ type: 'heading', level: Math.min(6, Math.max(1, t.depth)) as 1 | 2 | 3 | 4 | 5 | 6, children });
        else if (children.length) out.push({ type: 'paragraph', children });
        break;
      }
      case 'paragraph': {
        const children = inlineOf(t.tokens, c);
        if (children.length) out.push({ type: 'paragraph', children });
        break;
      }
      case 'text': {
        // Lines of text that follow each other are one paragraph, a line end between them.
        const lines: Piece[] = pieces([t], c, false);
        while (i + 1 < tokens.length && (tokens[i + 1] as Token).type === 'text') {
          lines.push({ type: 'text', text: '\n' });
          lines.push(...pieces([tokens[++i] as Token], c, false));
        }
        const children = tidy(lines, c, false);
        if (children.some(n => n.type !== 'text' || n.text.trim())) out.push({ type: 'paragraph', children });
        break;
      }
      case 'blockquote': {
        const children = blocksOf(t.tokens, c, TOP) as Block[];
        if (!inside) out.push({ type: 'quote', children });
        else out.push(...children);
        break;
      }
      case 'list': {
        const items = t.items.map((item): ListItem => ({ checked: item.task ? !!item.checked : null, children: blocksOf(item.tokens, c, { loose: item.loose, item: true }) }));
        if (!inside) out.push({ type: 'list', ordered: t.ordered, start: t.ordered && typeof t.start === 'number' ? t.start : 1, items });
        else for (const item of items) out.push(...(item.children.map(b => (b.type === 'inline' ? { type: 'paragraph', children: b.children } : b)) as Block[]));
        break;
      }
      case 'table':
        if (inside) break;
        out.push({ type: 'table', align: t.align, head: t.header.map(cell => inlineOf(cell.tokens, c)), rows: t.rows.map(row => row.map(cell => inlineOf(cell.tokens, c))) });
        break;
      default: break;
    }
  }
  flush();
  return out;
}

/** The tree of a reply. */
export function parse(source: string, options: MarkdownOptions = {}): ReadonlyArray<Block> {
  const c: Context = { home: options.home ?? '', dropping: null };
  return blocksOf(lex(String(source ?? '')), c, TOP) as Block[];
}

// ------------------------------------------------------------------ as words

function inlineWords(nodes: ReadonlyArray<Inline>): string {
  let out = '';
  for (const n of nodes) {
    if (n.type === 'text' || n.type === 'long') out += n.text;
    else if (n.type === 'code') out += n.parts.map(p => p.text).join('');
    else if (n.type === 'break') out += '\n';
    else out += inlineWords(n.children);
  }
  return out;
}

function blockWords(nodes: ReadonlyArray<Block | InlineRun>, out: string[]): void {
  for (const b of nodes) {
    switch (b.type) {
      case 'paragraph': case 'heading': case 'inline': out.push(inlineWords(b.children)); break;
      case 'code': out.push(b.text); break;
      case 'rule': break;
      case 'quote': blockWords(b.children, out); break;
      case 'list': for (const item of b.items) blockWords(item.children, out); break;
      case 'table':
        out.push(b.head.map(inlineWords).join(' | '));
        for (const row of b.rows) out.push(row.map(inlineWords).join(' | '));
        break;
    }
  }
}

/** The words of a tree, a line for each block: for a preview, a notification, a search. */
export function plainText(tree: ReadonlyArray<Block>): string {
  const out: string[] = [];
  blockWords(tree, out);
  return out.join('\n');
}
