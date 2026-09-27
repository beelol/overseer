// How VS Code reads Markdown, so the phone reads it the same way.
//
// This is the lexer of marked 14.0.0 (the version the extension ships in media/vendor), in the
// configuration the extension uses: GitHub's flavour, single line ends do not break, no
// extensions. Its rules and the order it tries them in are kept as they are, because what a line
// of Markdown means is decided by them and by nothing else. Only the lexer is here: the phone has
// no page to render into, so src/markdown.ts turns these tokens into a tree instead of HTML.
//
// marked v14.0.0 - a markdown parser
// Copyright (c) 2011-2024, Christopher Jeffrey. (MIT Licensed)
// https://github.com/markedjs/marked
// The licence is in licenses/marked-LICENSE.md.
//
// test/markdown.test.ts compares the result with the real marked, input by input.

export interface Cell { text: string; tokens: Token[]; header: boolean; align: 'left' | 'right' | 'center' | null }
export interface ItemToken { type: 'list_item'; raw: string; task: boolean; checked: boolean | undefined; loose: boolean; text: string; tokens: Token[] }

export type Token =
  | { type: 'space'; raw: string }
  | { type: 'code'; raw: string; text: string; lang?: string | undefined }
  | { type: 'heading'; raw: string; depth: number; text: string; tokens: Token[] }
  | { type: 'hr'; raw: string }
  | { type: 'blockquote'; raw: string; text: string; tokens: Token[] }
  | { type: 'list'; raw: string; ordered: boolean; start: number | ''; loose: boolean; items: ItemToken[] }
  | { type: 'html'; raw: string; text: string; block: boolean }
  | { type: 'table'; raw: string; header: Cell[]; align: Array<'left' | 'right' | 'center' | null>; rows: Cell[][] }
  | { type: 'paragraph'; raw: string; text: string; tokens: Token[] }
  | { type: 'text'; raw: string; text: string; tokens?: Token[] }
  | { type: 'escape'; raw: string; text: string }
  | { type: 'link'; raw: string; href: string; title: string | null; text: string; tokens: Token[]; auto?: boolean }
  | { type: 'image'; raw: string; href: string; title: string | null; text: string }
  | { type: 'strong' | 'em' | 'del'; raw: string; text: string; tokens: Token[] }
  | { type: 'codespan'; raw: string; text: string }
  | { type: 'br'; raw: string };

interface Definition { tag: string; raw: string; href: string; title: string | undefined }
type Links = Record<string, { href: string; title: string | undefined }>;

// ------------------------------------------------------------------ helpers

const caret = /(^|[^[])\^/g;

function edit(regex: string | RegExp, opt = ''): { replace(name: string | RegExp, val: string | RegExp): ReturnType<typeof edit>; getRegex(): RegExp } {
  let source = typeof regex === 'string' ? regex : regex.source;
  const obj = {
    replace: (name: string | RegExp, val: string | RegExp) => {
      let valSource = typeof val === 'string' ? val : val.source;
      valSource = valSource.replace(caret, '$1');
      source = source.replace(name, valSource);
      return obj;
    },
    getRegex: () => new RegExp(source, opt),
  };
  return obj;
}

export function splitCells(tableRow: string, count?: number): string[] {
  // Every pipe that ends a cell gets a space before it, to tell it from an escaped one.
  const row = tableRow.replace(/\|/g, (_match, offset: number, str: string) => {
    let escaped = false;
    let curr = offset;
    while (--curr >= 0 && str[curr] === '\\') escaped = !escaped;
    return escaped ? '|' : ' |';
  });
  const cells = row.split(/ \|/);
  if (!(cells[0] as string).trim()) cells.shift();
  if (cells.length > 0 && !(cells[cells.length - 1] as string).trim()) cells.pop();
  if (count) {
    if (cells.length > count) cells.splice(count);
    else while (cells.length < count) cells.push('');
  }
  return cells.map(cell => cell.trim().replace(/\\\|/g, '|'));
}

function rtrim(str: string, c: string): string {
  let end = str.length;
  while (end > 0 && str.charAt(end - 1) === c) end--;
  return str.slice(0, end);
}

function findClosingBracket(str: string, b: string): number {
  if (str.indexOf(b[1] as string) === -1) return -1;
  let level = 0;
  for (let i = 0; i < str.length; i++) {
    if (str[i] === '\\') i++;
    else if (str[i] === b[0]) level++;
    else if (str[i] === b[1]) {
      level--;
      if (level < 0) return i;
    }
  }
  return -1;
}

function indentCodeCompensation(raw: string, text: string): string {
  const matchIndentToCode = raw.match(/^(\s+)(?:```)/);
  if (matchIndentToCode === null) return text;
  const indentToCode = matchIndentToCode[1] as string;
  return text.split('\n').map(node => {
    const matchIndentInNode = node.match(/^\s+/);
    if (matchIndentInNode === null) return node;
    const [indentInNode] = matchIndentInNode;
    return indentInNode.length >= indentToCode.length ? node.slice(indentToCode.length) : node;
  }).join('\n');
}

// ------------------------------------------------------------------ block grammar (GitHub's flavour)

const newline = /^(?: *(?:\n|$))+/;
const blockCode = /^( {4}[^\n]+(?:\n(?: *(?:\n|$))*)?)+/;
const fences = /^ {0,3}(`{3,}(?=[^`\n]*(?:\n|$))|~{3,})([^\n]*)(?:\n|$)(?:|([\s\S]*?)(?:\n|$))(?: {0,3}\1[~`]* *(?=\n|$)|$)/;
const hr = /^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)/;
const heading = /^ {0,3}(#{1,6})(?=\s|$)(.*)(?:\n+|$)/;
const bullet = /(?:[*+-]|\d{1,9}[.)])/;
const lheading = edit(/^(?!bull |blockCode|fences|blockquote|heading|html)((?:.|\n(?!\s*?\n|bull |blockCode|fences|blockquote|heading|html))+?)\n {0,3}(=+|-+) *(?:\n+|$)/)
  .replace(/bull/g, bullet)
  .replace(/blockCode/g, / {4}/)
  .replace(/fences/g, / {0,3}(?:`{3,}|~{3,})/)
  .replace(/blockquote/g, / {0,3}>/)
  .replace(/heading/g, / {0,3}#{1,6}/)
  .replace(/html/g, / {0,3}<[^\n>]+>\n/)
  .getRegex();
const _paragraph = /^([^\n]+(?:\n(?!hr|heading|lheading|blockquote|fences|list|html|table| +\n)[^\n]+)*)/;
const blockText = /^[^\n]+/;
const _blockLabel = /(?!\s*\])(?:\\.|[^[\]\\])+/;
const def = edit(/^ {0,3}\[(label)\]: *(?:\n *)?([^<\s][^\s]*|<.*?>)(?:(?: +(?:\n *)?| *\n *)(title))? *(?:\n+|$)/)
  .replace('label', _blockLabel)
  .replace('title', /(?:"(?:\\"?|[^"\\])*"|'[^'\n]*(?:\n[^'\n]+)*\n?'|\([^()]*\))/)
  .getRegex();
const list = edit(/^( {0,3}bull)([ \t][^\n]+?)?(?:\n|$)/).replace(/bull/g, bullet).getRegex();
const _tag = 'address|article|aside|base|basefont|blockquote|body|caption'
  + '|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption'
  + '|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe'
  + '|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option'
  + '|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title'
  + '|tr|track|ul';
const _comment = /<!--(?:-?>|[\s\S]*?(?:-->|$))/;
const html = edit('^ {0,3}(?:'
  + '<(script|pre|style|textarea)[\\s>][\\s\\S]*?(?:</\\1>[^\\n]*\\n+|$)'
  + '|comment[^\\n]*(\\n+|$)'
  + '|<\\?[\\s\\S]*?(?:\\?>\\n*|$)'
  + '|<![A-Z][\\s\\S]*?(?:>\\n*|$)'
  + '|<!\\[CDATA\\[[\\s\\S]*?(?:\\]\\]>\\n*|$)'
  + '|</?(tag)(?: +|\\n|/?>)[\\s\\S]*?(?:(?:\\n *)+\\n|$)'
  + '|<(?!script|pre|style|textarea)([a-z][\\w-]*)(?:attribute)*? */?>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n *)+\\n|$)'
  + '|</(?!script|pre|style|textarea)[a-z][\\w-]*\\s*>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n *)+\\n|$)'
  + ')', 'i')
  .replace('comment', _comment)
  .replace('tag', _tag)
  .replace('attribute', / +[a-zA-Z:_][\w.:-]*(?: *= *"[^"\n]*"| *= *'[^'\n]*'| *= *[^\s"'=<>`]+)?/)
  .getRegex();
const gfmTable = edit('^ *([^\\n ].*)\\n'
  + ' {0,3}((?:\\| *)?:?-+:? *(?:\\| *:?-+:? *)*(?:\\| *)?)'
  + '(?:\\n((?:(?! *\\n|hr|heading|blockquote|code|fences|list|html).*(?:\\n|$))*)\\n*|$)')
  .replace('hr', hr)
  .replace('heading', ' {0,3}#{1,6}(?:\\s|$)')
  .replace('blockquote', ' {0,3}>')
  .replace('code', ' {4}[^\\n]')
  .replace('fences', ' {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n')
  .replace('list', ' {0,3}(?:[*+-]|1[.)]) ')
  .replace('html', '</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)')
  .replace('tag', _tag)
  .getRegex();
const paragraph = edit(_paragraph)
  .replace('hr', hr)
  .replace('heading', ' {0,3}#{1,6}(?:\\s|$)')
  .replace('|lheading', '')
  .replace('table', gfmTable)
  .replace('blockquote', ' {0,3}>')
  .replace('fences', ' {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n')
  .replace('list', ' {0,3}(?:[*+-]|1[.)]) ')
  .replace('html', '</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)')
  .replace('tag', _tag)
  .getRegex();
const blockquote = edit(/^( {0,3}> ?(paragraph|[^\n]*)(?:\n|$))+/)
  // The grammar without tables (marked builds this rule before it adds them).
  .replace('paragraph', edit(_paragraph)
    .replace('hr', hr)
    .replace('heading', ' {0,3}#{1,6}(?:\\s|$)')
    .replace('|lheading', '')
    .replace('|table', '')
    .replace('blockquote', ' {0,3}>')
    .replace('fences', ' {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n')
    .replace('list', ' {0,3}(?:[*+-]|1[.)]) ')
    .replace('html', '</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)')
    .replace('tag', _tag)
    .getRegex())
  .getRegex();

// ------------------------------------------------------------------ inline grammar (GitHub's flavour)

const _punctuation = '\\p{P}\\p{S}';
const punctuation = edit(/^((?![*_])[\spunctuation])/, 'u').replace(/punctuation/g, _punctuation).getRegex();
const blockSkip = /\[[^[\]]*?\]\([^()]*?\)|`[^`]*?`|<[^<>]*?>/g;
const emStrongLDelim = edit(/^(?:\*+(?:((?!\*)[punct])|[^\s*]))|^_+(?:((?!_)[punct])|([^\s_]))/, 'u').replace(/punct/g, _punctuation).getRegex();
const emStrongRDelimAst = edit('^[^_*]*?__[^_*]*?\\*[^_*]*?(?=__)'
  + '|[^*]+(?=[^*])'
  + '|(?!\\*)[punct](\\*+)(?=[\\s]|$)'
  + '|[^punct\\s](\\*+)(?!\\*)(?=[punct\\s]|$)'
  + '|(?!\\*)[punct\\s](\\*+)(?=[^punct\\s])'
  + '|[\\s](\\*+)(?!\\*)(?=[punct])'
  + '|(?!\\*)[punct](\\*+)(?!\\*)(?=[punct])'
  + '|[^punct\\s](\\*+)(?=[^punct\\s])', 'gu')
  .replace(/punct/g, _punctuation)
  .getRegex();
const emStrongRDelimUnd = edit('^[^_*]*?\\*\\*[^_*]*?_[^_*]*?(?=\\*\\*)'
  + '|[^_]+(?=[^_])'
  + '|(?!_)[punct](_+)(?=[\\s]|$)'
  + '|[^punct\\s](_+)(?!_)(?=[punct\\s]|$)'
  + '|(?!_)[punct\\s](_+)(?=[^punct\\s])'
  + '|[\\s](_+)(?!_)(?=[punct])'
  + '|(?!_)[punct](_+)(?!_)(?=[punct])', 'gu')
  .replace(/punct/g, _punctuation)
  .getRegex();
export const anyPunctuation = edit(/\\([punct])/, 'gu').replace(/punct/g, _punctuation).getRegex();
const autolink = edit(/^<(scheme:[^\s\x00-\x1f<>]*|email)>/)
  .replace('scheme', /[a-zA-Z][a-zA-Z0-9+.-]{1,31}/)
  .replace('email', /[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+(@)[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(?![-_])/)
  .getRegex();
const _inlineComment = edit(_comment).replace('(?:-->|$)', '-->').getRegex();
const tag = edit('^comment'
  + '|^</[a-zA-Z][\\w:-]*\\s*>'
  + '|^<[a-zA-Z][\\w-]*(?:attribute)*?\\s*/?>'
  + '|^<\\?[\\s\\S]*?\\?>'
  + '|^<![a-zA-Z]+\\s[\\s\\S]*?>'
  + '|^<!\\[CDATA\\[[\\s\\S]*?\\]\\]>')
  .replace('comment', _inlineComment)
  .replace('attribute', /\s+[a-zA-Z:_][\w.:-]*(?:\s*=\s*"[^"]*"|\s*=\s*'[^']*'|\s*=\s*[^\s"'=<>`]+)?/)
  .getRegex();
const _inlineLabel = /(?:\[(?:\\.|[^[\]\\])*\]|\\.|`[^`]*`|[^[\]\\`])*?/;
const link = edit(/^!?\[(label)\]\(\s*(href)(?:\s+(title))?\s*\)/)
  .replace('label', _inlineLabel)
  .replace('href', /<(?:\\.|[^\n<>\\])+>|[^\s\x00-\x1f]*/)
  .replace('title', /"(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\])*'|\((?:\\\)?|[^)\\])*\)/)
  .getRegex();
const reflink = edit(/^!?\[(label)\]\[(ref)\]/).replace('label', _inlineLabel).replace('ref', _blockLabel).getRegex();
const nolink = edit(/^!?\[(ref)\](?:\[\])?/).replace('ref', _blockLabel).getRegex();
const reflinkSearch = edit('reflink|nolink(?!\\()', 'g').replace('reflink', reflink).replace('nolink', nolink).getRegex();
const escape = edit(/^\\([!"#$%&'()*+,\-./:;<=>?@[\]\\^_`{|}~])/).replace('])', '~|])').getRegex();
const inlineCode = /^(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/;
const br = /^( {2,}|\\)\n(?!\s*$)/;
const url = edit(/^((?:ftp|https?):\/\/|www\.)(?:[a-zA-Z0-9-]+\.?)+[^\s<]*|^email/, 'i')
  .replace('email', /[A-Za-z0-9._+-]+(@)[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![-_])/)
  .getRegex();
const backpedal = /(?:[^?!.,:;*_'"~()&]+|\([^)]*\)|&(?![a-zA-Z0-9]+;$)|[?!.,:;*_'"~)]+(?!$))+/;
const del = /^(~~?)(?=[^\s~])([\s\S]*?[^\s~])\1(?=[^~]|$)/;
const inlineText = /^([`~]+|[^`~])(?:(?= {2,}\n)|(?=[a-zA-Z0-9.!#$%&'*+/=?_`{|}~-]+@)|[\s\S]*?(?:(?=[\\<![`*~_]|\b_|https?:\/\/|ftp:\/\/|www\.|$)|[^ ](?= {2,}\n)|[^a-zA-Z0-9.!#$%&'*+/=?_`{|}~-](?=[a-zA-Z0-9.!#$%&'*+/=?_`{|}~-]+@)))/;

// ------------------------------------------------------------------ the lexer

interface State { inLink: boolean; inRawBlock: boolean; top: boolean }

class Lexer {
  tokens: Token[] = [];
  links: Links = Object.create(null) as Links;
  state: State = { inLink: false, inRawBlock: false, top: true };
  inlineQueue: Array<{ src: string; tokens: Token[] }> = [];

  lex(src: string): Token[] {
    this.blockTokens(src.replace(/\r\n|\r/g, '\n'), this.tokens);
    for (let i = 0; i < this.inlineQueue.length; i++) {
      const next = this.inlineQueue[i] as { src: string; tokens: Token[] };
      this.inlineTokens(next.src, next.tokens);
    }
    this.inlineQueue = [];
    return this.tokens;
  }

  inline(src: string, tokens: Token[] = []): Token[] {
    this.inlineQueue.push({ src, tokens });
    return tokens;
  }

  private requeue(text: string): void {
    (this.inlineQueue[this.inlineQueue.length - 1] as { src: string }).src = text;
  }

  blockTokens(source: string, tokens: Token[] = [], lastParagraphClipped = false): Token[] {
    let src = source.replace(/^( *)(\t+)/gm, (_all, leading: string, tabs: string) => leading + '    '.repeat(tabs.length));
    let token: Token | undefined;
    while (src) {
      if ((token = this.space(src))) {
        src = src.substring(token.raw.length);
        // A single line end closes the last line: it is not a gap between blocks.
        if (token.raw.length === 1 && tokens.length > 0) (tokens[tokens.length - 1] as Token).raw += '\n';
        else tokens.push(token);
        continue;
      }
      if ((token = this.code(src))) {
        src = src.substring(token.raw.length);
        const last = tokens[tokens.length - 1];
        // Indented code cannot interrupt a paragraph.
        if (last && (last.type === 'paragraph' || last.type === 'text')) {
          last.raw += '\n' + token.raw;
          last.text += '\n' + (token as { text: string }).text;
          this.requeue(last.text);
        } else tokens.push(token);
        continue;
      }
      if ((token = this.fences(src) || this.heading(src) || this.hr(src) || this.blockquote(src) || this.list(src) || this.html(src))) {
        src = src.substring(token.raw.length);
        tokens.push(token);
        continue;
      }
      const defined = this.def(src);
      if (defined) {
        src = src.substring(defined.raw.length);
        const last = tokens[tokens.length - 1];
        if (last && (last.type === 'paragraph' || last.type === 'text')) {
          last.raw += '\n' + defined.raw;
          last.text += '\n' + defined.raw;
          this.requeue(last.text);
        } else if (!this.links[defined.tag]) this.links[defined.tag] = { href: defined.href, title: defined.title };
        continue;
      }
      if ((token = this.table(src) || this.lheading(src))) {
        src = src.substring(token.raw.length);
        tokens.push(token);
        continue;
      }
      if (this.state.top && (token = this.paragraph(src))) {
        const last = tokens[tokens.length - 1];
        if (lastParagraphClipped && last?.type === 'paragraph') {
          last.raw += '\n' + token.raw;
          last.text += '\n' + (token as { text: string }).text;
          this.inlineQueue.pop();
          this.requeue(last.text);
        } else tokens.push(token);
        lastParagraphClipped = false;
        src = src.substring(token.raw.length);
        continue;
      }
      if ((token = this.text(src))) {
        src = src.substring(token.raw.length);
        const last = tokens[tokens.length - 1];
        if (last && last.type === 'text') {
          last.raw += '\n' + token.raw;
          last.text += '\n' + (token as { text: string }).text;
          this.inlineQueue.pop();
          this.requeue(last.text);
        } else tokens.push(token);
        continue;
      }
      // Nothing matched: the rules cover every input, so this does not happen; stop rather than loop.
      break;
    }
    this.state.top = true;
    return tokens;
  }

  // ---- blocks

  private space(src: string): Token | undefined {
    const cap = newline.exec(src);
    return cap && cap[0].length > 0 ? { type: 'space', raw: cap[0] } : undefined;
  }

  private code(src: string): Token | undefined {
    const cap = blockCode.exec(src);
    if (!cap) return undefined;
    return { type: 'code', raw: cap[0], text: rtrim(cap[0].replace(/^ {1,4}/gm, ''), '\n') };
  }

  private fences(src: string): Token | undefined {
    const cap = fences.exec(src);
    if (!cap) return undefined;
    return { type: 'code', raw: cap[0], lang: cap[2] ? cap[2].trim().replace(anyPunctuation, '$1') : cap[2], text: indentCodeCompensation(cap[0], cap[3] || '') };
  }

  private heading(src: string): Token | undefined {
    const cap = heading.exec(src);
    if (!cap) return undefined;
    let text = (cap[2] as string).trim();
    if (/#$/.test(text)) {
      const trimmed = rtrim(text, '#');
      // CommonMark wants a space before the closing marks.
      if (!trimmed || / $/.test(trimmed)) text = trimmed.trim();
    }
    return { type: 'heading', raw: cap[0], depth: (cap[1] as string).length, text, tokens: this.inline(text) };
  }

  private hr(src: string): Token | undefined {
    const cap = hr.exec(src);
    return cap ? { type: 'hr', raw: rtrim(cap[0], '\n') } : undefined;
  }

  private blockquote(src: string): (Token & { type: 'blockquote' }) | undefined {
    const cap = blockquote.exec(src);
    if (!cap) return undefined;
    let lines = rtrim(cap[0], '\n').split('\n');
    let raw = '';
    let text = '';
    const tokens: Token[] = [];
    while (lines.length > 0) {
      let inBlockquote = false;
      const currentLines: string[] = [];
      let i;
      for (i = 0; i < lines.length; i++) {
        // Lines up to a continuation.
        if (/^ {0,3}>/.test(lines[i] as string)) {
          currentLines.push(lines[i] as string);
          inBlockquote = true;
        } else if (!inBlockquote) currentLines.push(lines[i] as string);
        else break;
      }
      lines = lines.slice(i);
      const currentRaw = currentLines.join('\n');
      const currentText = currentRaw
        // A continuation that looks like a heading's underline is kept from being one.
        .replace(/\n {0,3}((?:=+|-+) *)(?=\n|$)/g, '\n    $1')
        .replace(/^ {0,3}>[ \t]?/gm, '');
      raw = raw ? `${raw}\n${currentRaw}` : currentRaw;
      text = text ? `${text}\n${currentText}` : currentText;
      const top = this.state.top;
      this.state.top = true;
      this.blockTokens(currentText, tokens, true);
      this.state.top = top;
      if (lines.length === 0) break;
      const last = tokens[tokens.length - 1];
      if (last?.type === 'code') break;
      if (last?.type === 'blockquote') {
        // The continuation belongs to the quote inside.
        const newToken = this.blockquote(last.raw + '\n' + lines.join('\n')) as Token & { type: 'blockquote' };
        tokens[tokens.length - 1] = newToken;
        raw = raw.substring(0, raw.length - last.raw.length) + newToken.raw;
        text = text.substring(0, text.length - last.text.length) + newToken.text;
        break;
      }
      if (last?.type === 'list') {
        // The continuation belongs to the list inside.
        const newText = last.raw + '\n' + lines.join('\n');
        const newToken = this.list(newText) as Token & { type: 'list' };
        tokens[tokens.length - 1] = newToken;
        raw = raw.substring(0, raw.length - last.raw.length) + newToken.raw;
        text = text.substring(0, text.length - last.raw.length) + newToken.raw;
        lines = newText.substring(newToken.raw.length).split('\n');
        continue;
      }
    }
    return { type: 'blockquote', raw, tokens, text };
  }

  private list(source: string): (Token & { type: 'list' }) | undefined {
    let src = source;
    let cap = list.exec(src);
    if (!cap) return undefined;
    let bull = (cap[1] as string).trim();
    const isordered = bull.length > 1;
    const out: Token & { type: 'list' } = { type: 'list', raw: '', ordered: isordered, start: isordered ? +bull.slice(0, -1) : '', loose: false, items: [] };
    bull = isordered ? `\\d{1,9}\\${bull.slice(-1)}` : `\\${bull}`;
    const itemRegex = new RegExp(`^( {0,3}${bull})((?:[\t ][^\\n]*)?(?:\\n|$))`);
    let endsWithBlankLine = false;
    while (src) {
      let endEarly = false;
      let raw = '';
      let itemContents = '';
      if (!(cap = itemRegex.exec(src))) break;
      // A bullet that is a rule ends the list.
      if (hr.test(src)) break;
      raw = cap[0];
      src = src.substring(raw.length);
      let line = ((cap[2] as string).split('\n', 1)[0] as string).replace(/^\t+/, t => ' '.repeat(3 * t.length));
      let nextLine = src.split('\n', 1)[0] as string;
      let blankLine = !line.trim();
      let indent = 0;
      if (blankLine) indent = (cap[1] as string).length + 1;
      else {
        indent = (cap[2] as string).search(/[^ ]/);
        // More than four spaces is code inside the item: one counts.
        indent = indent > 4 ? 1 : indent;
        itemContents = line.slice(indent);
        indent += (cap[1] as string).length;
      }
      if (blankLine && /^ *$/.test(nextLine)) {
        // An item begins with at most one empty line.
        raw += nextLine + '\n';
        src = src.substring(nextLine.length + 1);
        endEarly = true;
      }
      if (!endEarly) {
        const lead = ` {0,${Math.min(3, indent - 1)}}`;
        const nextBulletRegex = new RegExp(`^${lead}(?:[*+-]|\\d{1,9}[.)])((?:[ \t][^\\n]*)?(?:\\n|$))`);
        const hrRegex = new RegExp(`^${lead}((?:- *){3,}|(?:_ *){3,}|(?:\\* *){3,})(?:\\n+|$)`);
        const fencesBeginRegex = new RegExp(`^${lead}(?:\`\`\`|~~~)`);
        const headingBeginRegex = new RegExp(`^${lead}#`);
        while (src) {
          const rawLine = src.split('\n', 1)[0] as string;
          nextLine = rawLine;
          if (fencesBeginRegex.test(nextLine) || headingBeginRegex.test(nextLine) || nextBulletRegex.test(nextLine) || hrRegex.test(src)) break;
          if (nextLine.search(/[^ ]/) >= indent || !nextLine.trim()) itemContents += '\n' + nextLine.slice(indent);
          else {
            // Not indented enough: it continues the paragraph, unless the last line was something else.
            if (blankLine) break;
            if (line.search(/[^ ]/) >= 4) break;
            if (fencesBeginRegex.test(line) || headingBeginRegex.test(line) || hrRegex.test(line)) break;
            itemContents += '\n' + nextLine;
          }
          if (!blankLine && !nextLine.trim()) blankLine = true;
          raw += rawLine + '\n';
          src = src.substring(rawLine.length + 1);
          line = nextLine.slice(indent);
        }
      }
      if (!out.loose) {
        // An item that ended with an empty line, followed by another, makes the list loose.
        if (endsWithBlankLine) out.loose = true;
        else if (/\n *\n *$/.test(raw)) endsWithBlankLine = true;
      }
      const istask = /^\[[ xX]\] /.exec(itemContents);
      let ischecked: boolean | undefined;
      if (istask) {
        ischecked = istask[0] !== '[ ] ';
        itemContents = itemContents.replace(/^\[[ xX]\] +/, '');
      }
      out.items.push({ type: 'list_item', raw, task: !!istask, checked: ischecked, loose: false, text: itemContents, tokens: [] });
      out.raw += raw;
    }
    const lastItem = out.items[out.items.length - 1] as ItemToken;
    lastItem.raw = lastItem.raw.trimEnd();
    lastItem.text = lastItem.text.trimEnd();
    out.raw = out.raw.trimEnd();
    for (const item of out.items) {
      this.state.top = false;
      item.tokens = this.blockTokens(item.text, []);
      if (!out.loose) {
        const spacers = item.tokens.filter(t => t.type === 'space');
        out.loose = spacers.length > 0 && spacers.some(t => /\n.*\n/.test(t.raw));
      }
    }
    if (out.loose) for (const item of out.items) item.loose = true;
    return out;
  }

  private html(src: string): Token | undefined {
    const cap = html.exec(src);
    return cap ? { type: 'html', block: true, raw: cap[0], text: cap[0] } : undefined;
  }

  private def(src: string): Definition | undefined {
    const cap = def.exec(src);
    if (!cap) return undefined;
    return {
      tag: (cap[1] as string).toLowerCase().replace(/\s+/g, ' '), raw: cap[0],
      href: cap[2] ? cap[2].replace(/^<(.*)>$/, '$1').replace(anyPunctuation, '$1') : '',
      title: cap[3] ? cap[3].substring(1, cap[3].length - 1).replace(anyPunctuation, '$1') : cap[3],
    };
  }

  private table(src: string): Token | undefined {
    const cap = gfmTable.exec(src);
    if (!cap) return undefined;
    // Without a pipe or a colon the second line underlines a heading.
    if (!/[:|]/.test(cap[2] as string)) return undefined;
    const headers = splitCells(cap[1] as string);
    const aligns = (cap[2] as string).replace(/^\||\| *$/g, '').split('|');
    const rows = cap[3] && cap[3].trim() ? cap[3].replace(/\n[ \t]*$/, '').split('\n') : [];
    if (headers.length !== aligns.length) return undefined;
    const align = aligns.map(a => (/^ *-+: *$/.test(a) ? 'right' : /^ *:-+: *$/.test(a) ? 'center' : /^ *:-+ *$/.test(a) ? 'left' : null));
    return {
      type: 'table', raw: cap[0], align,
      header: headers.map((text, i) => ({ text, tokens: this.inline(text), header: true, align: align[i] ?? null })),
      rows: rows.map(row => splitCells(row, headers.length).map((text, i) => ({ text, tokens: this.inline(text), header: false, align: align[i] ?? null }))),
    };
  }

  private lheading(src: string): Token | undefined {
    const cap = lheading.exec(src);
    if (!cap) return undefined;
    return { type: 'heading', raw: cap[0], depth: (cap[2] as string).charAt(0) === '=' ? 1 : 2, text: cap[1] as string, tokens: this.inline(cap[1] as string) };
  }

  private paragraph(src: string): Token | undefined {
    const cap = paragraph.exec(src);
    if (!cap) return undefined;
    const body = cap[1] as string;
    const text = body.charAt(body.length - 1) === '\n' ? body.slice(0, -1) : body;
    return { type: 'paragraph', raw: cap[0], text, tokens: this.inline(text) };
  }

  private text(src: string): Token | undefined {
    const cap = blockText.exec(src);
    return cap ? { type: 'text', raw: cap[0], text: cap[0], tokens: this.inline(cap[0]) } : undefined;
  }

  // ---- inline

  inlineTokens(source: string, tokens: Token[] = []): Token[] {
    let src = source;
    let token: Token | undefined;
    // The text with links, code and escapes masked, so they do not disturb emphasis.
    let maskedSrc = src;
    let match: RegExpExecArray | null;
    let keepPrevChar = false;
    let prevChar = '';
    const names = Object.keys(this.links);
    if (names.length > 0) {
      while ((match = reflinkSearch.exec(maskedSrc)) !== null) {
        if (names.includes(match[0].slice(match[0].lastIndexOf('[') + 1, -1))) {
          maskedSrc = maskedSrc.slice(0, match.index) + '[' + 'a'.repeat(match[0].length - 2) + ']' + maskedSrc.slice(reflinkSearch.lastIndex);
        }
      }
    }
    while ((match = blockSkip.exec(maskedSrc)) !== null) {
      maskedSrc = maskedSrc.slice(0, match.index) + '[' + 'a'.repeat(match[0].length - 2) + ']' + maskedSrc.slice(blockSkip.lastIndex);
    }
    while ((match = anyPunctuation.exec(maskedSrc)) !== null) {
      maskedSrc = maskedSrc.slice(0, match.index) + '++' + maskedSrc.slice(anyPunctuation.lastIndex);
    }
    while (src) {
      if (!keepPrevChar) prevChar = '';
      keepPrevChar = false;
      if ((token = this.escape(src))) {
        src = src.substring(token.raw.length);
        tokens.push(token);
        continue;
      }
      if ((token = this.tag(src)) || (token = this.link(src))) {
        src = src.substring(token.raw.length);
        tokens.push(token);
        continue;
      }
      if ((token = this.reflink(src))) {
        src = src.substring(token.raw.length);
        const last = tokens[tokens.length - 1];
        if (last && token.type === 'text' && last.type === 'text') {
          last.raw += token.raw;
          last.text += token.text;
        } else tokens.push(token);
        continue;
      }
      if ((token = this.emStrong(src, maskedSrc, prevChar) || this.codespan(src) || this.br(src) || this.del(src) || this.autolink(src))) {
        src = src.substring(token.raw.length);
        tokens.push(token);
        continue;
      }
      if (!this.state.inLink && (token = this.url(src))) {
        src = src.substring(token.raw.length);
        tokens.push(token);
        continue;
      }
      if ((token = this.inlineText(src))) {
        src = src.substring(token.raw.length);
        // The character before a run of underscores is remembered.
        if (token.raw.slice(-1) !== '_') prevChar = token.raw.slice(-1);
        keepPrevChar = true;
        const last = tokens[tokens.length - 1];
        if (last && last.type === 'text') {
          last.raw += token.raw;
          last.text += (token as { text: string }).text;
        } else tokens.push(token);
        continue;
      }
      break;
    }
    return tokens;
  }

  private escape(src: string): Token | undefined {
    const cap = escape.exec(src);
    return cap ? { type: 'escape', raw: cap[0], text: cap[1] as string } : undefined;
  }

  private tag(src: string): Token | undefined {
    const cap = tag.exec(src);
    if (!cap) return undefined;
    if (!this.state.inLink && /^<a /i.test(cap[0])) this.state.inLink = true;
    else if (this.state.inLink && /^<\/a>/i.test(cap[0])) this.state.inLink = false;
    if (!this.state.inRawBlock && /^<(pre|code|kbd|script)(\s|>)/i.test(cap[0])) this.state.inRawBlock = true;
    else if (this.state.inRawBlock && /^<\/(pre|code|kbd|script)(\s|>)/i.test(cap[0])) this.state.inRawBlock = false;
    return { type: 'html', raw: cap[0], block: false, text: cap[0] };
  }

  private outputLink(cap: RegExpExecArray, to: { href: string; title: string | undefined }, raw: string): Token {
    const text = (cap[1] as string).replace(/\\([[\]])/g, '$1');
    if (cap[0].charAt(0) !== '!') {
      this.state.inLink = true;
      const token: Token = { type: 'link', raw, href: to.href, title: to.title ? to.title : null, text, tokens: this.inlineTokens(text) };
      this.state.inLink = false;
      return token;
    }
    return { type: 'image', raw, href: to.href, title: to.title ? to.title : null, text };
  }

  private link(src: string): Token | undefined {
    const cap = link.exec(src);
    if (!cap) return undefined;
    const trimmedUrl = (cap[2] as string).trim();
    if (/^</.test(trimmedUrl)) {
      // Angle brackets must match, and the closing one cannot be escaped.
      if (!/>$/.test(trimmedUrl)) return undefined;
      const rtrimSlash = rtrim(trimmedUrl.slice(0, -1), '\\');
      if ((trimmedUrl.length - rtrimSlash.length) % 2 === 0) return undefined;
    } else {
      const lastParenIndex = findClosingBracket(cap[2] as string, '()');
      if (lastParenIndex > -1) {
        const start = cap[0].indexOf('!') === 0 ? 5 : 4;
        const linkLen = start + (cap[1] as string).length + lastParenIndex;
        cap[2] = (cap[2] as string).substring(0, lastParenIndex);
        cap[0] = cap[0].substring(0, linkLen).trim();
        cap[3] = '';
      }
    }
    let href = (cap[2] as string).trim();
    const title = cap[3] ? cap[3].slice(1, -1) : '';
    if (/^</.test(href)) href = href.slice(1, -1);
    return this.outputLink(cap, { href: href ? href.replace(anyPunctuation, '$1') : href, title: title ? title.replace(anyPunctuation, '$1') : title }, cap[0]);
  }

  private reflink(src: string): Token | undefined {
    const cap = reflink.exec(src) || nolink.exec(src);
    if (!cap) return undefined;
    const linkString = ((cap[2] || cap[1]) as string).replace(/\s+/g, ' ');
    const to = this.links[linkString.toLowerCase()];
    if (!to) {
      const text = cap[0].charAt(0);
      return { type: 'text', raw: text, text };
    }
    return this.outputLink(cap, to, cap[0]);
  }

  private emStrong(src: string, masked: string, prevChar: string): Token | undefined {
    let match = emStrongLDelim.exec(src);
    if (!match) return undefined;
    // An underscore cannot sit between two letters or digits.
    if (match[3] && prevChar.match(/[\p{L}\p{N}]/u)) return undefined;
    const nextChar = match[1] || match[2] || '';
    if (!nextChar || !prevChar || punctuation.exec(prevChar)) {
      const lLength = [...match[0]].length - 1;
      let rDelim: string | undefined, rLength: number, delimTotal = lLength, midDelimTotal = 0;
      const endReg = match[0][0] === '*' ? emStrongRDelimAst : emStrongRDelimUnd;
      endReg.lastIndex = 0;
      const maskedSrc = masked.slice(-1 * src.length + lLength);
      while ((match = endReg.exec(maskedSrc)) !== null) {
        rDelim = match[1] || match[2] || match[3] || match[4] || match[5] || match[6];
        if (!rDelim) continue;
        rLength = [...rDelim].length;
        if (match[3] || match[4]) {
          // Another opening mark.
          delimTotal += rLength;
          continue;
        } else if (match[5] || match[6]) {
          // Could open or close: CommonMark's rules 9 and 10.
          if (lLength % 3 && !((lLength + rLength) % 3)) {
            midDelimTotal += rLength;
            continue;
          }
        }
        delimTotal -= rLength;
        if (delimTotal > 0) continue;
        rLength = Math.min(rLength, rLength + delimTotal + midDelimTotal);
        const lastCharLength = ([...match[0]][0] as string).length;
        const raw = src.slice(0, lLength + match.index + lastCharLength + rLength);
        if (Math.min(lLength, rLength) % 2) {
          const text = raw.slice(1, -1);
          return { type: 'em', raw, text, tokens: this.inlineTokens(text) };
        }
        const text = raw.slice(2, -2);
        return { type: 'strong', raw, text, tokens: this.inlineTokens(text) };
      }
    }
    return undefined;
  }

  private codespan(src: string): Token | undefined {
    const cap = inlineCode.exec(src);
    if (!cap) return undefined;
    let text = (cap[2] as string).replace(/\n/g, ' ');
    if (/[^ ]/.test(text) && /^ /.test(text) && / $/.test(text)) text = text.substring(1, text.length - 1);
    return { type: 'codespan', raw: cap[0], text };
  }

  private br(src: string): Token | undefined {
    const cap = br.exec(src);
    return cap ? { type: 'br', raw: cap[0] } : undefined;
  }

  private del(src: string): Token | undefined {
    const cap = del.exec(src);
    return cap ? { type: 'del', raw: cap[0], text: cap[2] as string, tokens: this.inlineTokens(cap[2] as string) } : undefined;
  }

  private autolink(src: string): Token | undefined {
    const cap = autolink.exec(src);
    if (!cap) return undefined;
    const text = cap[1] as string;
    return { type: 'link', raw: cap[0], text, href: cap[2] === '@' ? 'mailto:' + text : text, title: null, tokens: [{ type: 'text', raw: text, text }], auto: true };
  }

  private url(src: string): Token | undefined {
    const cap = url.exec(src);
    if (!cap) return undefined;
    let text: string, href: string;
    if (cap[2] === '@') {
      text = cap[0];
      href = 'mailto:' + text;
    } else {
      // What an address does not end with is taken off its end.
      let prevCapZero;
      do {
        prevCapZero = cap[0];
        cap[0] = backpedal.exec(cap[0])?.[0] ?? '';
      } while (prevCapZero !== cap[0]);
      text = cap[0];
      href = cap[1] === 'www.' ? 'http://' + cap[0] : cap[0];
    }
    return { type: 'link', raw: cap[0], text, href, title: null, tokens: [{ type: 'text', raw: text, text }], auto: true };
  }

  private inlineText(src: string): Token | undefined {
    const cap = inlineText.exec(src);
    return cap ? { type: 'text', raw: cap[0], text: cap[0] } : undefined;
  }
}

/** The tokens of a Markdown text, as marked 14 reads it. */
export function lex(source: string): Token[] {
  return new Lexer().lex(source);
}
