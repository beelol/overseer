// The agent's replies as a small tree, so the app can show them with its own views.
//
// VS Code renders replies with marked (GitHub's flavour, single line ends do not break), cleans
// the result with DOMPurify and shortens long unbroken tokens (extension/media/markdown.js). This
// reads the same Markdown into the same structure: paragraphs, headings, lists (with tasks),
// tables, code blocks with their language, quotes, rules; and inside them text, strong, emphasis,
// strike-through, inline code, links, line breaks and shortened long tokens.
//
// Raw HTML never becomes a view. A tag is dropped and its text kept, as the sanitizer does for a
// tag it does not allow; what a script, style or similar element holds is dropped with it. A link
// keeps its address only when the sanitizer would keep it, and opens only when VS Code would
// open it (http and https).
//
// test/markdown.test.ts renders the same inputs with the extension's real files and compares.

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

const ESCAPABLE = /\\([!"#$%&'()*+,\-./:;<=>?@[\]\\^_`{|}~])/g;
const unescape = (s: string): string => decodeEntities(s.replace(ESCAPABLE, '$1'));

// ------------------------------------------------------------------ raw HTML

const COMMENT = /^<!--(?:-?>|[\s\S]*?(?:-->|$))/;
const ATTRIBUTE = '\\s+[a-zA-Z:_][\\w.:-]*(?:\\s*=\\s*"[^"]*"|\\s*=\\s*\'[^\']*\'|\\s*=\\s*[^\\s"\'=<>`]+)?';
const CLOSE_TAG = /^<\/([a-zA-Z][\w:-]*)\s*>/;
const OPEN_TAG = new RegExp(`^<([a-zA-Z][\\w-]*)(?:${ATTRIBUTE})*?\\s*(/?)>`);
const OTHER_TAG = /^<\?[\s\S]*?\?>|^<![a-zA-Z]+\s[\s\S]*?>|^<!\[CDATA\[[\s\S]*?\]\]>/;

/** Elements the sanitizer removes with everything they hold. */
const DROPPED_WITH_CONTENT = new Set(['annotation-xml', 'audio', 'colgroup', 'desc', 'foreignobject', 'head', 'iframe', 'math', 'mi', 'mn', 'mo', 'ms', 'mtext', 'noembed', 'noframes', 'noscript', 'plaintext', 'script', 'style', 'svg', 'template', 'title', 'video', 'xmp']);
/** Elements that hold text, not markup: an entity in them is not read. */
const RAW_TEXT = new Set(['script', 'style', 'xmp', 'iframe', 'noembed', 'noframes', 'plaintext', 'noscript']);

const BLOCK_TAGS = 'address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul';
const HTML_BLOCK: ReadonlyArray<{ start: RegExp; end: RegExp | null; interrupts: boolean }> = [
  { start: /^ {0,3}<(?:script|pre|style|textarea)(?:\s|>|$)/i, end: /<\/(?:script|pre|style|textarea)>/i, interrupts: true },
  { start: /^ {0,3}<!--/, end: /-->/, interrupts: true },
  { start: /^ {0,3}<\?/, end: /\?>/, interrupts: true },
  { start: /^ {0,3}<![A-Z]/, end: />/, interrupts: true },
  { start: /^ {0,3}<!\[CDATA\[/, end: /\]\]>/, interrupts: true },
  { start: new RegExp(`^ {0,3}</?(?:${BLOCK_TAGS})(?: +|\\n|/?>|$)`, 'i'), end: null, interrupts: true },
  { start: new RegExp(`^ {0,3}(?:<[a-zA-Z][\\w-]*(?:${ATTRIBUTE})*?\\s*/?>|</[a-zA-Z][\\w:-]*\\s*>)[ \\t]*$`), end: null, interrupts: false },
];

// ------------------------------------------------------------------ the parser's own state

interface Definition { readonly href: string; readonly title: string | null }

interface Context {
  readonly home: string;
  readonly definitions: Map<string, Definition>;
  /** The element whose end is waited for: everything until then is dropped. */
  dropping: string | null;
}

/** Blocks as first read: what they say is still text. */
type Draft =
  | { type: 'paragraph'; raw: string }
  | { type: 'text'; raw: string }
  | { type: 'heading'; level: 1 | 2 | 3 | 4 | 5 | 6; raw: string }
  | { type: 'code'; language: string; text: string }
  | { type: 'rule' }
  | { type: 'quote'; children: Draft[] }
  | { type: 'list'; ordered: boolean; start: number; loose: boolean; items: Array<{ checked: boolean | null; children: Draft[] }> }
  | { type: 'table'; align: Array<'left' | 'right' | 'center' | null>; head: string[]; rows: string[][] }
  | { type: 'html'; raw: string };

const BLANK = /^[ \t]*$/;
const FENCE = /^( {0,3})(`{3,}(?=[^`]*$)|~{3,})(.*)$/;
const HEADING = /^ {0,3}(#{1,6})(?=\s|$)(.*)$/;
const HR = /^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})$/;
const QUOTE = /^ {0,3}>/;
const BULLET = /^( {0,3}(?:[*+-]|\d{1,9}[.)]))((?:[ \t].*)?)$/;
const LHEADING = /^ {0,3}(=+|-+) *$/;
const DEFINITION = /^ {0,3}\[((?:\\.|[^[\]\\])+)\]: *(?:\n *)?(<[^\n<>]*>|[^\s<][^\s]*)(?:(?: +(?:\n *)?| *\n *)("(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\\n])*(?:\n[^'\n]+)*\n?'|\((?:\\\)?|[^)\\])*\)))? *(?:\n|$)/;
const TABLE_ALIGN = /^ {0,3}((?:\| *)?:?-+:? *(?:\| *:?-+:? *)*(?:\| *)?)$/;

function htmlStart(line: string, interrupting: boolean): { end: RegExp | null } | undefined {
  for (const kind of HTML_BLOCK) if ((!interrupting || kind.interrupts) && kind.start.test(line)) return { end: kind.end };
  return undefined;
}

/** True when a line, in the middle of a paragraph, begins something else. */
function interrupts(lines: ReadonlyArray<string>, i: number): boolean {
  const line = lines[i] as string;
  if (HR.test(line) || HEADING.test(line) || QUOTE.test(line) || FENCE.test(line)) return true;
  if (/^ {0,3}(?:[*+-]|1[.)]) /.test(line)) return true;
  if (/^ {0,3}<(?:\/?(?:address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul)(?: +|\n|\/?>|$)|(?:script|pre|style|textarea|!--))/i.test(line)) return true;
  return tableAt(lines, i) !== undefined;
}

/** marked's `splitCells`: the cells of a table's row. */
function splitCells(row: string, count?: number): string[] {
  const spaced = row.replace(/\|/g, (_match, offset: number, whole: string) => {
    let escaped = false;
    for (let at = offset - 1; at >= 0 && whole[at] === '\\'; at--) escaped = !escaped;
    return escaped ? '|' : ' |';
  });
  const cells = spaced.split(/ \|/);
  if (!(cells[0] as string).trim()) cells.shift();
  if (cells.length > 0 && !(cells[cells.length - 1] as string).trim()) cells.pop();
  if (count !== undefined) {
    if (cells.length > count) cells.splice(count);
    else while (cells.length < count) cells.push('');
  }
  return cells.map(cell => cell.trim().replace(/\\\|/g, '|'));
}

function tableAt(lines: ReadonlyArray<string>, i: number): { head: string[]; align: Array<'left' | 'right' | 'center' | null> } | undefined {
  const header = lines[i], rule = lines[i + 1];
  if (header === undefined || rule === undefined || !/^ *[^\n ]/.test(header)) return undefined;
  const m = TABLE_ALIGN.exec(rule);
  if (!m || !/[:|]/.test(m[1] as string)) return undefined;
  const head = splitCells(header);
  const aligns = (m[1] as string).replace(/^\||\| *$/g, '').split('|');
  if (head.length !== aligns.length) return undefined;
  return { head, align: aligns.map(a => (/^ *-+: *$/.test(a) ? 'right' : /^ *:-+: *$/.test(a) ? 'center' : /^ *:-+ *$/.test(a) ? 'left' : null)) };
}

/** Lines into blocks. `top` is false inside a list item, where plain lines are text, not paragraphs. */
function blocks(lines: ReadonlyArray<string>, c: Context, top: boolean): Draft[] {
  const out: Draft[] = [];
  let i = 0;
  let spaced = false;
  const last = (): Draft | undefined => out[out.length - 1];
  while (i < lines.length) {
    const line = lines[i] as string;
    if (BLANK.test(line)) { i++; spaced = true; continue; }
    const afterSpace = spaced;
    spaced = false;

    // Code, by four spaces.
    if (/^ {4}/.test(line)) {
      const before = last();
      if (before !== undefined && (before.type === 'paragraph' || before.type === 'text') && !afterSpace) {
        before.raw += '\n' + line;
        i++;
        continue;
      }
      const held: string[] = [];
      while (i < lines.length && (/^ {4}/.test(lines[i] as string) || BLANK.test(lines[i] as string))) {
        if (BLANK.test(lines[i] as string)) {
          // Empty lines belong to the code only when more code follows.
          let next = i;
          while (next < lines.length && BLANK.test(lines[next] as string)) next++;
          if (next >= lines.length || !/^ {4}/.test(lines[next] as string)) break;
        }
        held.push((lines[i] as string).replace(/^(?: {1,4}| {0,3}\t)/, ''));
        i++;
      }
      out.push({ type: 'code', language: '', text: held.join('\n').replace(/\n+$/, '') });
      continue;
    }

    // Code, between fences.
    const fence = FENCE.exec(line);
    if (fence) {
      const [, indent = '', marks = '', info = ''] = fence;
      const closing = new RegExp(`^ {0,3}${marks[0] === '`' ? '`' : '~'}{${marks.length},}[~\`]* *$`);
      const held: string[] = [];
      i++;
      while (i < lines.length && !closing.test(lines[i] as string)) { held.push(lines[i] as string); i++; }
      i++;
      const dedent = indent.length ? new RegExp(`^ {1,${indent.length}}`) : null;
      out.push({ type: 'code', language: info.trim().replace(ESCAPABLE, '$1'), text: held.map(l => (dedent ? l.replace(dedent, '') : l)).join('\n') });
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      let words = (heading[2] as string).trim();
      if (/#$/.test(words)) {
        const trimmed = words.replace(/#+$/, '');
        if (!trimmed || / $/.test(trimmed)) words = trimmed.trim();
      }
      out.push({ type: 'heading', level: (heading[1] as string).length as 1 | 2 | 3 | 4 | 5 | 6, raw: words });
      i++;
      continue;
    }

    if (HR.test(line)) { out.push({ type: 'rule' }); i++; continue; }

    if (QUOTE.test(line)) {
      const held: string[] = [];
      let lazy = false;
      while (i < lines.length) {
        const l = lines[i] as string;
        if (QUOTE.test(l)) { held.push(l.replace(/^ {0,3}> ?/, '')); lazy = !BLANK.test(held[held.length - 1] as string); i++; continue; }
        // A line without the mark continues the quote's paragraph.
        if (!lazy || BLANK.test(l) || HR.test(l) || HEADING.test(l) || FENCE.test(l) || BULLET.test(l) || htmlStart(l, true)) break;
        const inner = held[held.length - 1] as string;
        if (FENCE.test(inner) || /^ {4}/.test(inner) || HEADING.test(inner) || HR.test(inner)) break;
        held.push(l);
        i++;
      }
      out.push({ type: 'quote', children: blocks(held, c, true) });
      continue;
    }

    const bullet = BULLET.exec(line);
    if (bullet && !HR.test(line)) {
      const made = list(lines, i, c);
      if (made !== undefined) {
        out.push(made.list);
        i = made.next;
        if (made.spaced) spaced = true;
        continue;
      }
    }

    const html = htmlStart(line, continues(last(), afterSpace));
    if (html !== undefined) {
      const held: string[] = [];
      if (html.end !== null) {
        while (i < lines.length) {
          held.push(lines[i] as string);
          if (html.end.test(lines[i++] as string)) break;
        }
      } else {
        while (i < lines.length && !BLANK.test(lines[i] as string)) held.push(lines[i++] as string);
      }
      out.push({ type: 'html', raw: held.join('\n') });
      continue;
    }

    // A definition for links written by name. It shows nothing.
    if (line.trimStart().startsWith('[')) {
      const rest = lines.slice(i, i + 3).join('\n');
      const def = DEFINITION.exec(rest);
      if (def) {
        const label = (def[1] as string).toLowerCase().replace(/\s+/g, ' ');
        const href = (def[2] as string).replace(/^<(.*)>$/, '$1').replace(ESCAPABLE, '$1');
        const title = def[3] !== undefined ? (def[3] as string).slice(1, -1).replace(ESCAPABLE, '$1') : null;
        if (!c.definitions.has(label)) c.definitions.set(label, { href, title });
        i += (def[0] as string).replace(/\n$/, '').split('\n').length;
        continue;
      }
    }

    const table = tableAt(lines, i);
    if (table !== undefined) {
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length) {
        const l = lines[i] as string;
        if (BLANK.test(l) || HR.test(l) || HEADING.test(l) || QUOTE.test(l) || /^ {4}/.test(l) || FENCE.test(l) || /^ {0,3}(?:[*+-]|\d{1,9}[.)]) /.test(l) || htmlStart(l, true)) break;
        rows.push(splitCells(l, table.head.length));
        i++;
      }
      out.push({ type: 'table', align: table.align, head: table.head, rows });
      continue;
    }

    // Lines under which a line of = or - is drawn are a heading.
    let under = -1;
    for (let k = i + 1; k < lines.length; k++) {
      const l = lines[k] as string;
      if (LHEADING.test(l)) { under = k; break; }
      if (BLANK.test(l) || /^ {0,3}(?:[*+-]|\d{1,9}[.)]) /.test(l) || /^ {4}[^\n]/.test(l) || FENCE.test(l) || QUOTE.test(l) || HEADING.test(l) || htmlStart(l, true)) break;
    }
    if (under > 0) {
      out.push({ type: 'heading', level: /=/.test(lines[under] as string) ? 1 : 2, raw: lines.slice(i, under).join('\n').trim() });
      i = under + 1;
      continue;
    }

    // Plain lines: a paragraph, or the text of a list item.
    const first = line.replace(/^ {1,3}/, '');
    i++;
    if (top) {
      const held = [first];
      while (i < lines.length && !BLANK.test(lines[i] as string) && !interrupts(lines, i)) held.push(lines[i++] as string);
      out.push({ type: 'paragraph', raw: held.join('\n') });
      continue;
    }
    const before = last();
    if (before !== undefined && before.type === 'text' && !afterSpace) before.raw += '\n' + first;
    else out.push({ type: 'text', raw: first });
  }
  return out;
}

const continues = (before: Draft | undefined, afterSpace: boolean): boolean => before !== undefined && (before.type === 'paragraph' || before.type === 'text') && !afterSpace;

/** A list, read the way marked reads it. */
function list(lines: ReadonlyArray<string>, from: number, c: Context): { list: Draft; next: number; spaced: boolean } | undefined {
  const first = BULLET.exec(lines[from] as string);
  if (!first) return undefined;
  const mark = (first[1] as string).trim();
  const ordered = mark.length > 1;
  const same = new RegExp(`^( {0,3}${ordered ? `\\d{1,9}\\${mark.slice(-1)}` : `\\${mark}`})((?:[\\t ].*)?)$`);
  const items: Array<{ checked: boolean | null; lines: string[] }> = [];
  let loose = false;
  let endedBlank = false;
  let i = from;
  let trailing = false;
  while (i < lines.length) {
    const cap = same.exec(lines[i] as string);
    if (!cap || HR.test(lines[i] as string)) break;
    i++;
    const head = cap[1] as string, rest = cap[2] as string;
    let text = rest.replace(/^\t+/, t => ' '.repeat(3 * t.length));
    let blank = !text.trim();
    let indent: number;
    const held: string[] = [];
    if (blank) {
      indent = head.length + 1;
      held.push('');
    } else {
      indent = rest.search(/[^ ]/);
      indent = indent > 4 ? 1 : indent;
      held.push(text.slice(indent));
      indent += head.length;
    }
    let sawBlank = false;
    if (blank && i < lines.length && BLANK.test(lines[i] as string)) {
      // An item begins with at most one empty line.
      i++;
      sawBlank = true;
    } else {
      const lead = ` {0,${Math.min(3, indent - 1)}}`;
      const nextBullet = new RegExp(`^${lead}(?:[*+-]|\\d{1,9}[.)])((?:[ \\t].*)?)$`);
      const rule = new RegExp(`^${lead}((?:- *){3,}|(?:_ *){3,}|(?:\\* *){3,})$`);
      const fences = new RegExp(`^${lead}(?:\`\`\`|~~~)`);
      const headings = new RegExp(`^${lead}#`);
      const htmls = new RegExp(`^${lead}<(?:[a-z].*>|!--)`, 'i');
      while (i < lines.length) {
        const raw = lines[i] as string;
        const next = raw.replace(/\t/g, '    ');
        if (fences.test(raw) || headings.test(raw) || htmls.test(raw) || nextBullet.test(raw) || rule.test(raw)) break;
        if (next.search(/[^ ]/) >= indent || !raw.trim()) {
          held.push(next.slice(indent));
        } else {
          if (blank) break;
          if (text.replace(/\t/g, '    ').search(/[^ ]/) >= 4) break;
          if (fences.test(text) || headings.test(text) || rule.test(text)) break;
          held.push(raw);
        }
        if (!blank && !raw.trim()) blank = true;
        if (!raw.trim()) sawBlank = true;
        i++;
        text = next.slice(indent);
      }
    }
    // Empty lines at the end of an item are between items, not in it.
    let ends = false;
    while (held.length > 1 && BLANK.test(held[held.length - 1] as string)) { held.pop(); ends = true; }
    if (sawBlank && held.length === 1 && BLANK.test(held[0] as string)) ends = true;
    if (!loose) {
      if (endedBlank) loose = true;
      else if (ends) endedBlank = true;
    }
    trailing = ends;
    let checked: boolean | null = null;
    const task = /^\[[ xX]\] /.exec(held[0] as string);
    if (task) {
      checked = task[0] !== '[ ] ';
      held[0] = (held[0] as string).replace(/^\[[ xX]\] +/, '');
    }
    items.push({ checked, lines: held });
  }
  if (!items.length) return undefined;
  const built = items.map(item => ({ checked: item.checked, children: blocks(item.lines, c, false), lines: item.lines }));
  if (!loose) {
    // Two blocks of one item with an empty line between them make the list loose.
    loose = built.some(item => {
      let seenBlock = false, gap = false;
      for (const l of item.lines) {
        if (BLANK.test(l)) { if (seenBlock) gap = true; continue; }
        if (gap) return true;
        seenBlock = true;
      }
      return false;
    });
  }
  const start = ordered ? parseInt(mark, 10) : 1;
  return { list: { type: 'list', ordered, start: Number.isFinite(start) ? start : 1, loose, items: built.map(({ checked, children }) => ({ checked, children })) }, next: i, spaced: trailing };
}

// ------------------------------------------------------------------ inline

type Piece =
  | { kind: 'text'; text: string }
  | { kind: 'node'; node: Inline }
  | { kind: 'cut' }
  | { kind: 'mark'; char: string; count: number; opens: boolean; closes: boolean; origin: number };

const PUNCTUATION = /[\p{P}\p{S}]/u;
const isSpace = (ch: string | undefined): boolean => ch === undefined || /\s/.test(ch);
const isPunctuation = (ch: string | undefined): boolean => ch !== undefined && PUNCTUATION.test(ch);

const AUTOLINK = /^<([a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^\s\x00-\x1f<>]*|[a-zA-Z0-9.!#$%&'*+/=?_`{|}~-]+(@)[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(?![-_]))>/;
const URL_START = /^((?:ftp|https?):\/\/|www\.)(?:[a-zA-Z0-9-]+\.?)+[^\s<]*/i;
const EMAIL = /^[A-Za-z0-9._+-]+(@)[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![-_])/;
const BACKPEDAL = /(?:[^?!.,:;*_'"~()&]+|\([^)]*\)|&(?![a-zA-Z0-9]+;$)|[?!.,:;*_'"~)]+(?!$))+/;
const EMAIL_LOCAL = /[a-zA-Z0-9.!#$%&'*+/=?_`{|}~-]/;

function linkNode(href: string, title: string | null, children: ReadonlyArray<Inline>): Inline[] {
  const clean = cleanUrl(href);
  // marked writes a link whose address it cannot encode as its text.
  if (clean === null) return [...children];
  const kept = safeHref(clean);
  return [{ type: 'link', href: kept, title, opens: opensExternally(kept), children }];
}

/** The end of the bracket that opens at `from`, or -1. */
function closingBracket(s: string, from: number): number {
  let depth = 0;
  for (let i = from; i < s.length; i++) {
    const ch = s[i];
    if (ch === '\\') { i++; continue; }
    if (ch === '`') {
      const run = /^`+/.exec(s.slice(i)) as RegExpExecArray;
      const close = s.indexOf(run[0], i + run[0].length);
      if (close >= 0) { i = close + run[0].length - 1; continue; }
      i += run[0].length - 1;
      continue;
    }
    if (ch === '[') depth++;
    else if (ch === ']' && --depth === 0) return i;
  }
  return -1;
}

/** What follows a link's text: `(address "title")`. */
function destination(s: string, from: number): { href: string; title: string | null; end: number } | undefined {
  if (s[from] !== '(') return undefined;
  let i = from + 1;
  while (i < s.length && /\s/.test(s[i] as string)) i++;
  let href = '';
  if (s[i] === '<') {
    const m = /^<((?:\\.|[^\n<>\\])*)>/.exec(s.slice(i));
    if (!m) return undefined;
    href = m[1] as string;
    i += m[0].length;
  } else {
    let depth = 0;
    const begin = i;
    for (; i < s.length; i++) {
      const ch = s[i] as string;
      if (ch === '\\' && i + 1 < s.length) { i++; continue; }
      if (/[\s\x00-\x1f]/.test(ch)) break;
      if (ch === '(') depth++;
      else if (ch === ')') { if (depth === 0) break; depth--; }
    }
    href = s.slice(begin, i);
  }
  let title: string | null = null;
  const gap = /^\s+/.exec(s.slice(i));
  if (gap) {
    const t = /^("(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\])*'|\((?:\\\)?|[^)\\])*\))/.exec(s.slice(i + gap[0].length));
    if (t) { title = (t[1] as string).slice(1, -1); i += gap[0].length + t[0].length; }
  }
  while (i < s.length && /\s/.test(s[i] as string)) i++;
  if (s[i] !== ')') return undefined;
  return { href: unescape(href.trim()), title: title === null ? null : unescape(title), end: i + 1 };
}

function inline(source: string, c: Context, inLink = false): Inline[] {
  const pieces: Piece[] = [];
  const s = source;
  let text = '';
  const flush = (): void => { if (text) { pieces.push({ kind: 'text', text }); text = ''; } };
  let i = 0;
  while (i < s.length) {
    if (c.dropping !== null) {
      const end = new RegExp(`</${c.dropping}\\s*>`, 'i').exec(s.slice(i));
      if (!end) { i = s.length; break; }
      i += end.index + end[0].length;
      c.dropping = null;
      flush();
      pieces.push({ kind: 'cut' });
      continue;
    }
    const ch = s[i] as string;
    const rest = s.slice(i);

    if (ch === '\\') {
      const next = s[i + 1];
      if (next !== undefined && /[!"#$%&'()*+,\-./:;<=>?@[\]\\^_`{|}~]/.test(next)) { text += next; i += 2; continue; }
      if (next === '\n' && /\S/.test(s.slice(i + 2))) { flush(); pieces.push({ kind: 'node', node: { type: 'break' } }); i += 2; while (s[i] === ' ') i++; continue; }
      text += ch; i++;
      continue;
    }

    if (ch === '`') {
      const run = (/^`+/.exec(rest) as RegExpExecArray)[0];
      let close = -1;
      for (let at = i + run.length; at < s.length;) {
        const found = s.indexOf('`', at);
        if (found < 0) break;
        const len = (/^`+/.exec(s.slice(found)) as RegExpExecArray)[0].length;
        if (len === run.length) { close = found; break; }
        at = found + len;
      }
      if (close < 0) { text += run; i += run.length; continue; }
      let code = s.slice(i + run.length, close).replace(/\n/g, ' ');
      if (/[^ ]/.test(code) && code.startsWith(' ') && code.endsWith(' ')) code = code.slice(1, -1);
      flush();
      pieces.push({ kind: 'node', node: { type: 'code', text: code, parts: [{ type: 'text', text: code }] } });
      i = close + run.length;
      continue;
    }

    if (ch === '<') {
      const auto = AUTOLINK.exec(rest);
      if (auto) {
        const shown = auto[1] as string;
        flush();
        pieces.push(...linkNode(auto[2] === '@' ? 'mailto:' + shown : shown, null, [{ type: 'text', text: shown }]).map(node => ({ kind: 'node', node }) as Piece));
        i += auto[0].length;
        continue;
      }
      const tag = COMMENT.exec(rest) || CLOSE_TAG.exec(rest) || OPEN_TAG.exec(rest) || OTHER_TAG.exec(rest);
      if (tag) {
        i += tag[0].length;
        flush();
        const opened = OPEN_TAG.exec(tag[0]);
        const name = opened ? (opened[1] as string).toLowerCase() : '';
        if (name === 'br') pieces.push({ kind: 'node', node: { type: 'break' } });
        else pieces.push({ kind: 'cut' });
        if (opened && !opened[2] && (DROPPED_WITH_CONTENT.has(name) || RAW_TEXT.has(name))) c.dropping = name;
        continue;
      }
      text += ch; i++;
      continue;
    }

    if (ch === '&') {
      const e = entityAt(rest.slice(0, 40));
      if (e !== undefined) { text += e.text; i += e.length; continue; }
      text += ch; i++;
      continue;
    }

    if (ch === '!' && s[i + 1] === '[') {
      const close = closingBracket(s, i + 1);
      if (close > 0) {
        const to = destination(s, close + 1);
        const named = to === undefined ? reference(s, i + 1, close, c) : undefined;
        if (to !== undefined || named !== undefined) {
          // A picture: the sanitizer removes it, and nothing is shown in its place.
          flush();
          pieces.push({ kind: 'cut' });
          i = to !== undefined ? to.end : (named as { end: number }).end;
          continue;
        }
      }
      text += ch; i++;
      continue;
    }

    if (ch === '[') {
      const close = closingBracket(s, i);
      if (close > 0) {
        const to = destination(s, close + 1);
        const named = to === undefined ? reference(s, i, close, c) : undefined;
        const found = to ?? named;
        if (found !== undefined) {
          flush();
          const label = s.slice(i + 1, close);
          const children = inline(label, c, true);
          pieces.push(...linkNode(found.href, found.title, children).map(node => ({ kind: 'node', node }) as Piece));
          i = found.end;
          continue;
        }
      }
      text += ch; i++;
      continue;
    }

    if (ch === '*' || ch === '_' || ch === '~') {
      const run = (new RegExp(`^\\${ch}+`).exec(rest) as RegExpExecArray)[0];
      const before = i > 0 ? [...s.slice(Math.max(0, i - 2), i)].pop() : undefined;
      const after = [...s.slice(i + run.length, i + run.length + 2)][0];
      const left = !isSpace(after) && (!isPunctuation(after) || isSpace(before) || isPunctuation(before));
      const right = !isSpace(before) && (!isPunctuation(before) || isSpace(after) || isPunctuation(after));
      let opens = left, closes = right;
      if (ch === '_') { opens = left && (!right || isPunctuation(before)); closes = right && (!left || isPunctuation(after)); }
      if (ch === '~' && run.length > 2) { text += run; i += run.length; continue; }
      flush();
      pieces.push({ kind: 'mark', char: ch, count: run.length, opens, closes, origin: run.length });
      i += run.length;
      continue;
    }

    if (ch === '\n') {
      const spaces = /( {2,})$/.exec(text);
      if (spaces && /\S/.test(s.slice(i + 1))) {
        text = text.slice(0, -spaces[0].length);
        flush();
        pieces.push({ kind: 'node', node: { type: 'break' } });
      } else {
        text = text.replace(/ +$/, '') + '\n';
      }
      i++;
      while (s[i] === ' ') i++;
      continue;
    }

    if (!inLink) {
      // marked looks for an address where its text stops: before http://, https://, ftp:// and www. in
      // small letters anywhere, and in any letters only where something else has just ended.
      const url = /^(?:https?:\/\/|ftp:\/\/|www\.)/.test(rest) || (!text && /^(?:https?:\/\/|ftp:\/\/|www\.)/i.test(rest)) ? URL_START.exec(rest) : null;
      if (url) {
        let shown = url[0];
        for (let was = ''; was !== shown;) { was = shown; shown = BACKPEDAL.exec(shown)?.[0] ?? ''; }
        if (shown) {
          flush();
          pieces.push(...linkNode(url[1] === 'www.' ? 'http://' + shown : shown, null, [{ type: 'text', text: shown }]).map(node => ({ kind: 'node', node }) as Piece));
          i += shown.length;
          continue;
        }
      }
      if (EMAIL_LOCAL.test(ch) && (i === 0 || !EMAIL_LOCAL.test(s[i - 1] as string))) {
        const mail = EMAIL.exec(rest);
        if (mail) {
          flush();
          pieces.push(...linkNode('mailto:' + mail[0], null, [{ type: 'text', text: mail[0] }]).map(node => ({ kind: 'node', node }) as Piece));
          i += mail[0].length;
          continue;
        }
      }
    }

    text += ch;
    i++;
  }
  flush();
  return tidy(emphasis(pieces), c, inLink);
}

/** `[text][name]`, `[name][]` or `[name]`, when the name was defined. */
function reference(s: string, open: number, close: number, c: Context): { href: string; title: string | null; end: number } | undefined {
  const label = s.slice(open + 1, close);
  const second = /^\[((?:\\.|[^[\]\\])*)\]/.exec(s.slice(close + 1));
  const name = (second && second[1] ? second[1] : label).toLowerCase().replace(/\s+/g, ' ');
  const def = c.definitions.get(name);
  if (def === undefined) return undefined;
  return { href: def.href, title: def.title, end: close + 1 + (second ? second[0].length : 0) };
}

type Mark = Extract<Piece, { kind: 'mark' }>;
type Entry = Mark | { kind: 'held'; node: Inline | { cut: true } };

/** Marks into strong, emphasis and strike-through, by CommonMark's rule for delimiter runs. */
function emphasis(pieces: Piece[]): Array<Inline | { cut: true }> {
  const items: Entry[] = pieces.map(p => (p.kind === 'mark' ? { ...p } : { kind: 'held', node: p.kind === 'text' ? { type: 'text', text: p.text } : p.kind === 'node' ? p.node : { cut: true } }));
  const held = (e: Entry): Inline | { cut: true } => (e.kind === 'held' ? e.node : { type: 'text', text: e.char.repeat(e.count) });
  const pairs = (opener: Mark, closer: Mark): boolean => {
    if (opener.char !== closer.char || !opener.opens) return false;
    if (closer.char === '~') return opener.count === closer.count;
    return !((opener.closes || closer.opens) && (opener.origin + closer.origin) % 3 === 0 && !(opener.origin % 3 === 0 && closer.origin % 3 === 0));
  };
  for (let k = 0; k < items.length;) {
    const closer = items[k] as Entry;
    if (closer.kind !== 'mark' || !closer.closes) { k++; continue; }
    let at = -1;
    for (let o = k - 1; o >= 0; o--) {
      const e = items[o] as Entry;
      if (e.kind === 'mark' && pairs(e, closer)) { at = o; break; }
    }
    if (at < 0) { k++; continue; }
    const opener = items[at] as Mark;
    const use = closer.char === '~' ? closer.count : opener.count >= 2 && closer.count >= 2 ? 2 : 1;
    const inner = clean(items.slice(at + 1, k).map(held));
    const node: Inline = closer.char === '~' ? { type: 'strike', children: inner } : use === 2 ? { type: 'strong', children: inner } : { type: 'emphasis', children: inner };
    items.splice(at + 1, k - at - 1, { kind: 'held', node });
    k = at + 2;
    opener.count -= use;
    closer.count -= use;
    if (opener.count === 0) { items.splice(at, 1); k--; }
    if (closer.count === 0) items.splice(k, 1);
  }
  return items.map(held);
}

/** Without the places where a tag was dropped. */
function clean(nodes: Array<Inline | { cut: true }>): Inline[] {
  return merge(nodes).filter((n): n is Inline => !('cut' in n));
}

/** Neighbouring texts become one, as they are one in the page; not across a dropped tag. */
function merge(nodes: Array<Inline | { cut: true }>): Array<Inline | { cut: true }> {
  const out: Array<Inline | { cut: true }> = [];
  for (const n of nodes) {
    const before = out[out.length - 1];
    if (!('cut' in n) && n.type === 'text' && before !== undefined && !('cut' in before) && before.type === 'text') out[out.length - 1] = { type: 'text', text: before.text + n.text };
    else if (!('cut' in n) && n.type === 'text' && !n.text) continue;
    else out.push(n);
  }
  return out;
}

const LONG = /(\S{60,})/;

/** Long unbroken tokens become short ones that keep the whole. */
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

function tidy(nodes: Array<Inline | { cut: true }>, c: Context, inLink: boolean): Inline[] {
  const out: Inline[] = [];
  for (const n of merge(nodes)) {
    if ('cut' in n) continue;
    if (n.type === 'text') out.push(...(inLink ? [n] : shorten(n.text, c.home)));
    else if (n.type === 'code') out.push(inLink ? n : { ...n, parts: shorten(n.text, c.home) });
    else if (n.type === 'strong' || n.type === 'emphasis' || n.type === 'strike') out.push({ ...n, children: tidy([...n.children], c, inLink) });
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

// ------------------------------------------------------------------ from drafts to the tree

/** Raw HTML as the text it leaves behind. */
function htmlText(raw: string, c: Context): string {
  let out = '';
  let i = 0;
  while (i < raw.length) {
    if (c.dropping !== null) {
      const end = new RegExp(`</${c.dropping}\\s*>`, 'i').exec(raw.slice(i));
      if (!end) return out;
      i += end.index + end[0].length;
      c.dropping = null;
      continue;
    }
    const rest = raw.slice(i);
    if (raw[i] === '<') {
      const tag = COMMENT.exec(rest) || CLOSE_TAG.exec(rest) || /^<([a-zA-Z][^\s/>]*)(?:"[^"]*"|'[^']*'|[^'">])*>/.exec(rest) || OTHER_TAG.exec(rest) || /^<[!?][^>]*>/.exec(rest);
      if (tag) {
        const opened = /^<([a-zA-Z][^\s/>]*)/.exec(tag[0]);
        const name = opened ? (opened[1] as string).toLowerCase() : '';
        if (opened && !/\/>$/.test(tag[0]) && (DROPPED_WITH_CONTENT.has(name) || RAW_TEXT.has(name))) c.dropping = name;
        if (/^(?:br|p|div|li|tr|h[1-6]|\/p|\/div|\/li|\/tr|\/h[1-6])$/.test(name || tag[0].slice(1, -1).trim().toLowerCase())) out += ' ';
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

function codeBlock(language: string, body: string): CodeBlock {
  const word = /^\S*/.exec(language)?.[0] ?? '';
  const label = /^[\w+-]+/.exec(word)?.[0];
  const lines = body.split('\n').length + 1;
  return { type: 'code', language: word, label: label || 'text', text: body, lines, collapsed: lines > 24 };
}

function finish(drafts: ReadonlyArray<Draft>, c: Context, loose: boolean): Array<Block | InlineRun> {
  const out: Array<Block | InlineRun> = [];
  for (const d of drafts) {
    switch (d.type) {
      case 'paragraph': {
        const children = inline(d.raw, c);
        if (children.length) out.push({ type: 'paragraph', children });
        break;
      }
      case 'text': {
        const children = inline(d.raw, c);
        if (!children.length) break;
        out.push(loose ? { type: 'paragraph', children } : { type: 'inline', children });
        break;
      }
      case 'heading':
        out.push({ type: 'heading', level: d.level, children: inline(d.raw, c) });
        break;
      case 'code':
        if (c.dropping === null) out.push(codeBlock(d.language, d.text));
        break;
      case 'rule':
        if (c.dropping === null) out.push({ type: 'rule' });
        break;
      case 'quote':
        out.push({ type: 'quote', children: finish(d.children, c, true) as Block[] });
        break;
      case 'list':
        out.push({ type: 'list', ordered: d.ordered, start: d.start, items: d.items.map(item => ({ checked: item.checked, children: finish(item.children, c, d.loose) })) });
        break;
      case 'table':
        out.push({ type: 'table', align: d.align, head: d.head.map(cell => inline(cell, c)), rows: d.rows.map(row => row.map(cell => inline(cell, c))) });
        break;
      case 'html': {
        const words = htmlText(d.raw, c).replace(/\s+/g, ' ').trim();
        if (words) out.push({ type: 'paragraph', children: shorten(words, c.home) });
        break;
      }
    }
  }
  return out;
}

/** The tree of a reply. */
export function parse(source: string, options: MarkdownOptions = {}): ReadonlyArray<Block> {
  const text = String(source ?? '').replace(/\r\n|\r/g, '\n').replace(/^( *)(\t+)/gm, (_all, lead: string, tabs: string) => lead + '    '.repeat(tabs.length));
  const c: Context = { home: options.home ?? '', definitions: new Map(), dropping: null };
  const drafts = blocks(text.split('\n'), c, true);
  return finish(drafts, c, true) as Block[];
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
