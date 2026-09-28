/**
 * Syntax colouring for a line of a diff: comments, strings, numbers and keywords, by language.
 * A hunk has no lines around it, so each line is read by itself: a comment or a string that
 * began on a line the hunk does not hold is not known to be one.
 *
 * `phone/model` has no tokenizer; this one belongs there once another screen needs it.
 */

export type TokenKind = 'plain' | 'comment' | 'string' | 'number' | 'keyword';

export interface Token {
  readonly kind: TokenKind;
  readonly text: string;
}

export interface Language {
  readonly name: string;
  /** What starts a comment that runs to the end of the line. */
  readonly line: readonly string[];
  /** What opens and closes a comment inside a line. */
  readonly block: readonly [string, string] | null;
  /** The characters that open and close a string. */
  readonly quotes: string;
  readonly keywords: ReadonlySet<string>;
  /** Keywords in any case (SQL). */
  readonly anyCase: boolean;
  /** A line that starts with `*` continues a comment (`/** … *\/`). */
  readonly starContinues: boolean;
}

const words = (list: string): ReadonlySet<string> => new Set(list.split(' '));

function language(name: string, options: { line?: string[]; block?: readonly [string, string]; quotes?: string; keywords: string; anyCase?: boolean; starContinues?: boolean }): Language {
  return { name, line: options.line ?? [], block: options.block ?? null, quotes: options.quotes ?? '"\'', keywords: words(options.keywords), anyCase: options.anyCase ?? false, starContinues: options.starContinues ?? false };
}

const C_BLOCK = ['/*', '*/'] as const;

const SCRIPT = language('script', {
  line: ['//'], block: C_BLOCK, quotes: '"\'`', starContinues: true,
  keywords: 'abstract any as async await boolean break case catch class const continue debugger declare default delete do else enum export extends false finally for from function get if implements import in infer instanceof interface is keyof let namespace never new null number of override private protected public readonly return satisfies set static string super switch this throw true try type typeof undefined unknown var void while with yield',
});
const RUST = language('rust', {
  line: ['//'], block: C_BLOCK, quotes: '"', starContinues: true,
  keywords: 'as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while',
});
const PYTHON = language('python', {
  line: ['#'],
  keywords: 'and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield match case',
});
const GO = language('go', {
  line: ['//'], block: C_BLOCK, quotes: '"\'`', starContinues: true,
  keywords: 'break case chan const continue default defer else fallthrough false for func go goto if import interface map nil package range return select struct switch true type var',
});
const C_LIKE = language('c', {
  line: ['//'], block: C_BLOCK, starContinues: true,
  keywords: 'abstract auto bool boolean break byte case catch char class const constexpr continue default delete do double else enum explicit extends extern false final finally float for friend goto if implements import inline instanceof int interface long namespace native new null nullptr operator override package private protected public register return short signed sizeof static struct super switch synchronized template this throw throws true try typedef typename union unsigned using var virtual void volatile while',
});
const SWIFT = language('swift', {
  line: ['//'], block: C_BLOCK, quotes: '"', starContinues: true,
  keywords: 'actor as associatedtype async await break case catch class continue default defer deinit do else enum extension fallthrough false fileprivate final for func guard if import in init inout internal is lazy let mutating nil open operator override private protocol public repeat rethrows return self Self some static struct subscript super switch throw throws true try typealias var weak where while',
});
const KOTLIN = language('kotlin', {
  line: ['//'], block: C_BLOCK, starContinues: true,
  keywords: 'abstract as break by catch class companion const continue data do else enum false final finally for fun if import in init inline interface internal is lateinit null object open operator override package private protected public return sealed super suspend this throw true try typealias val var when while',
});
const RUBY = language('ruby', {
  line: ['#'],
  keywords: 'alias and begin break case class def defined? do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield require',
});
const SHELL = language('shell', {
  line: ['#'],
  keywords: 'case do done elif else esac export fi for function if in local return select set then until while',
});
const SQL = language('sql', {
  line: ['--'], block: C_BLOCK, quotes: "'", anyCase: true,
  keywords: 'add all alter and as asc begin between by case check column commit constraint create cross default delete desc distinct drop else end exists foreign from full group having if in index inner insert into is join key left like limit not null on or order outer primary references right rollback select set table then union unique update values view when where with',
});
const DATA = language('data', { line: [], keywords: 'true false null' });
const CONFIG = language('config', { line: ['#'], keywords: 'true false null yes no on off' });
const STYLE = language('style', { block: C_BLOCK, keywords: 'important inherit initial none unset', starContinues: true });
const MARKUP = language('markup', { block: ['<!--', '-->'], keywords: '' });

const BY_ENDING: Readonly<Record<string, Language>> = {
  ts: SCRIPT, tsx: SCRIPT, mts: SCRIPT, cts: SCRIPT, js: SCRIPT, jsx: SCRIPT, mjs: SCRIPT, cjs: SCRIPT,
  rs: RUST, py: PYTHON, pyi: PYTHON, go: GO,
  c: C_LIKE, h: C_LIKE, cc: C_LIKE, cpp: C_LIKE, cxx: C_LIKE, hpp: C_LIKE, m: C_LIKE, mm: C_LIKE, java: C_LIKE, cs: C_LIKE, dart: C_LIKE, php: C_LIKE,
  swift: SWIFT, kt: KOTLIN, kts: KOTLIN, gradle: KOTLIN, rb: RUBY,
  sh: SHELL, bash: SHELL, zsh: SHELL, fish: SHELL,
  sql: SQL, json: DATA, jsonc: SCRIPT, json5: SCRIPT,
  yml: CONFIG, yaml: CONFIG, toml: CONFIG, ini: CONFIG, conf: CONFIG, env: CONFIG,
  css: STYLE, scss: STYLE, less: STYLE,
  html: MARKUP, htm: MARKUP, xml: MARKUP, svg: MARKUP, plist: MARKUP, vue: MARKUP,
};

const BY_NAME: Readonly<Record<string, Language>> = { dockerfile: CONFIG, makefile: CONFIG, gemfile: RUBY, rakefile: RUBY, podfile: RUBY };

/** The language of a file by its name, or `null` when it is not one this knows. */
export function languageOf(path: string): Language | null {
  const name = (path.split('/').pop() ?? '').toLowerCase();
  const dot = name.lastIndexOf('.');
  if (dot > 0) return BY_ENDING[name.slice(dot + 1)] ?? null;
  return BY_NAME[name.replace(/^\./, '')] ?? null;
}

const isDigit = (c: string): boolean => c >= '0' && c <= '9';
const startsWord = (c: string): boolean => (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c === '_' || c === '$';
const inWord = (c: string): boolean => startsWord(c) || isDigit(c);

/** A line as tokens. Joined, the tokens are the line. */
export function tokenize(line: string, lang: Language | null): readonly Token[] {
  if (lang === null || line.length === 0) return line.length === 0 ? [] : [{ kind: 'plain', text: line }];
  if (lang.starContinues && /^\s*\*(\s|\/|$)/.test(line)) return [{ kind: 'comment', text: line }];
  const out: Token[] = [];
  let plain = '';
  const push = (kind: TokenKind, text: string): void => {
    if (kind === 'plain') {
      plain += text;
      return;
    }
    if (plain) out.push({ kind: 'plain', text: plain });
    plain = '';
    out.push({ kind, text });
  };
  let at = 0;
  while (at < line.length) {
    const c = line[at] as string;
    if (lang.line.some((mark) => line.startsWith(mark, at)) && (c !== '#' || at === 0 || /\s/.test(line[at - 1] as string))) {
      push('comment', line.slice(at));
      break;
    }
    if (lang.block !== null && line.startsWith(lang.block[0], at)) {
      const close = line.indexOf(lang.block[1], at + lang.block[0].length);
      const end = close < 0 ? line.length : close + lang.block[1].length;
      push('comment', line.slice(at, end));
      at = end;
      continue;
    }
    if (lang.quotes.includes(c)) {
      let end = at + 1;
      while (end < line.length && line[end] !== c) end += line[end] === '\\' ? 2 : 1;
      end = Math.min(line.length, end + 1);
      push('string', line.slice(at, end));
      at = end;
      continue;
    }
    if (isDigit(c) && (at === 0 || !inWord(line[at - 1] as string))) {
      let end = at + 1;
      while (end < line.length && (inWord(line[end] as string) || (line[end] === '.' && isDigit(line[end + 1] ?? '')))) end += 1;
      push('number', line.slice(at, end));
      at = end;
      continue;
    }
    if (startsWord(c)) {
      let end = at + 1;
      while (end < line.length && inWord(line[end] as string)) end += 1;
      const word = line.slice(at, end);
      push(lang.keywords.has(lang.anyCase ? word.toLowerCase() : word) ? 'keyword' : 'plain', word);
      at = end;
      continue;
    }
    push('plain', c);
    at += 1;
  }
  if (plain) out.push({ kind: 'plain', text: plain });
  return out;
}

/**
 * The tokens of a line, cut where the line is broken into `pieces` (`review.splitLine`): one
 * list of tokens for each piece.
 */
export function cut(tokens: readonly Token[], pieces: readonly string[]): readonly (readonly Token[])[] {
  if (pieces.length <= 1) return [tokens];
  const out: Token[][] = [];
  let index = 0;
  let used = 0;
  for (const piece of pieces) {
    const row: Token[] = [];
    let left = piece.length;
    while (left > 0 && index < tokens.length) {
      const token = tokens[index] as Token;
      const rest = token.text.length - used;
      const take = Math.min(rest, left);
      row.push({ kind: token.kind, text: token.text.slice(used, used + take) });
      left -= take;
      used += take;
      if (used >= token.text.length) {
        index += 1;
        used = 0;
      }
    }
    out.push(row);
  }
  return out;
}
