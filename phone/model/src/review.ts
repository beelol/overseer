// Review on the phone: the changed files, the comparison to choose, and a file's hunks as rows.
//
// The daemon sends hunks without context: the lines a hunk removed and the lines it added, with
// where each starts. A hunk here is its removed lines, then its added lines, each with its line
// number on its own side, and whether the hunk is marked reviewed. Marks are the daemon's: one
// made in VS Code shows here and the reverse (`review_mark` events keep the store's copy current).

import { PHONE_ONLY, TEXT } from './text.ts';
import type { Change, Comparison, Hunk } from './types.ts';

export type StatusLetter = 'A' | 'D' | 'R' | 'M' | 'U';

/** The letter VS Code's review shows for a change the daemon reports (extension/src/review.js). */
export function statusLetter(status: string): StatusLetter {
  switch (status) {
    case 'A': case 'C': return 'A';
    case 'D': return 'D';
    case 'R': return 'R';
    default: return 'M';
  }
}

export interface FileRow {
  readonly kind: 'folder' | 'file';
  /** A folder's path with a "/" at its end, or a file's path. */
  readonly key: string;
  readonly depth: number;
  readonly name: string;
  readonly path: string;
  readonly status: StatusLetter | null;
  readonly statusText: string | null;
  readonly oldPath: string | null;
  readonly conflicted: boolean;
  /** Lines added and removed, hunks and how many are reviewed: `null` until the file's hunks are known. */
  readonly added: number | null;
  readonly removed: number | null;
  readonly hunks: number | null;
  readonly reviewed: number | null;
  readonly tooltip: string;
  readonly accessibilityLabel: string;
  /** A folder that shows what it holds. Always true for a file. */
  readonly expanded: boolean;
  readonly icon: string;
}

export interface FilesOptions {
  /** Only files whose path holds this. */
  readonly query?: string;
  /** Folders closed by the owner, by key. */
  readonly collapsed?: ReadonlySet<string>;
  /** By path, for the files whose hunks were fetched. */
  readonly hunks?: Readonly<Record<string, ReadonlyArray<Hunk>>>;
  /** Files with conflicts (the workspace's status). They stay in the list. */
  readonly conflicted?: ReadonlyArray<string>;
}

interface Folder { dirs: Map<string, Folder>; files: Array<{ change: Change; conflicted: boolean }> }

/** The changed files, flat, grouped by folder the way VS Code's tree groups them: folders first, then files, in the daemon's order. */
export function changedFiles(changes: ReadonlyArray<Change>, options: FilesOptions = {}): ReadonlyArray<FileRow> {
  const query = (options.query ?? '').toLocaleLowerCase();
  const conflicted = new Set(options.conflicted ?? []);
  const entries = changes.map(change => ({ change, conflicted: conflicted.has(change.path) }));
  // A conflicted file stays in the list even when the comparison does not include it.
  for (const path of conflicted) if (!changes.some(c => c.path === path)) entries.push({ change: { status: 'U', path, old_path: null }, conflicted: true });
  const root: Folder = { dirs: new Map(), files: [] };
  for (const entry of entries) {
    if (!entry.change.path.toLocaleLowerCase().includes(query)) continue;
    const parts = entry.change.path.split('/');
    parts.pop();
    let dir = root;
    for (const part of parts) {
      let next = dir.dirs.get(part);
      if (next === undefined) dir.dirs.set(part, next = { dirs: new Map(), files: [] });
      dir = next;
    }
    dir.files.push(entry);
  }
  const out: FileRow[] = [];
  const fill = (dir: Folder, prefix: string, depth: number): void => {
    for (const [name, child] of dir.dirs) {
      const key = prefix + name + '/';
      const expanded = !options.collapsed?.has(key) || !!query;
      out.push({ kind: 'folder', key, depth, name, path: key, status: null, statusText: null, oldPath: null, conflicted: false, added: null, removed: null, hunks: null, reviewed: null, tooltip: key, accessibilityLabel: name, expanded, icon: 'folder' });
      if (expanded) fill(child, key, depth + 1);
    }
    for (const { change, conflicted: clash } of dir.files) {
      const letter = statusLetter(change.status);
      const known = options.hunks?.[change.path];
      out.push({
        kind: 'file', key: change.path, depth, name: change.path.split('/').pop() as string, path: change.path, status: letter,
        statusText: clash ? PHONE_ONLY.conflicted : letter === 'A' ? PHONE_ONLY.newFile : letter === 'D' ? PHONE_ONLY.deletedFile : letter === 'R' && change.old_path ? PHONE_ONLY.renamedFrom(change.old_path) : null,
        oldPath: change.old_path ?? null, conflicted: clash,
        added: known ? known.reduce((n, h) => n + h.modified_lines.length, 0) : null, removed: known ? known.reduce((n, h) => n + h.base_lines.length, 0) : null,
        hunks: known ? known.length : null, reviewed: known ? known.filter(h => h.reviewed).length : null,
        tooltip: change.path + (clash ? ` (${TEXT.review.conflicted})` : ''), accessibilityLabel: [change.path, letter, clash && TEXT.review.conflicted].filter(Boolean).join(', '),
        expanded: true, icon: clash ? 'warning' : 'file',
      });
    }
  };
  fill(root, '', 0);
  return out;
}

export interface ChangesSummary {
  readonly files: number;
  /** "3 files" */
  readonly text: string;
  /** "+10", or empty when the daemon did not count. */
  readonly added: string;
  /** "−2" */
  readonly removed: string;
  /** "a.ts, b.ts, c.ts, …" */
  readonly names: string;
  readonly tooltip: string;
}

/** The changes bar under a conversation (chat.js `changes`); `null` when nothing changed. */
export function changesSummary(changes: { files: number; added?: number; removed?: number; names?: ReadonlyArray<string> } | null | undefined): ChangesSummary | null {
  const n = (changes && changes.files) || 0;
  if (!changes || !n) return null;
  const names = changes.names ?? [];
  return {
    files: n, text: TEXT.chat.files(n), added: changes.added !== undefined ? TEXT.conversation.added(changes.added) : '', removed: changes.added !== undefined ? TEXT.conversation.removed(changes.removed ?? 0) : '',
    names: names.slice(0, 3).join(', ') + (names.length > 3 ? ', …' : ''), tooltip: TEXT.chat.reviewChanges + '\n' + names.join('\n'),
  };
}

export interface ComparisonChoice {
  readonly key: string;
  /** latest_run, turn:2, task_start, fork, branch_merge_base, branch_tip; 'other' for choosing another branch. */
  readonly mode: string;
  readonly branch: string | null;
  /** As the daemon says it, which is what VS Code shows: "Latest run", "Since task start". */
  readonly label: string;
  /** The first ten characters of the comparison's commit, or "unavailable". */
  readonly description: string;
  readonly detail: string;
  readonly available: boolean;
  readonly selected: boolean;
  readonly base: string | null;
}

const choiceKey = (mode: string, branch: string | null | undefined): string => `${mode}:${branch ?? ''}`;

/** The comparisons to choose from, as VS Code's picker lists them, ending with "Other branch…". */
export function comparisonChoices(options: ReadonlyArray<Comparison>, selected?: { mode: string; branch?: string | null }): ReadonlyArray<ComparisonChoice> {
  const chosen = selected !== undefined ? options.find(o => o.mode === selected.mode && (!selected.branch || o.branch === selected.branch) && o.available) : undefined;
  const current = chosen ?? options.find(o => o.default && o.available) ?? options.find(o => o.available);
  const out: ComparisonChoice[] = options.map(o => ({
    key: choiceKey(o.mode, o.branch), mode: o.mode, branch: o.branch ?? null, label: o.label, description: o.available ? (o.base || '').slice(0, 10) : TEXT.review.unavailable,
    detail: o.detail || '', available: o.available, selected: o === current, base: o.base ?? null,
  }));
  out.push({ key: 'other', mode: 'other', branch: null, label: TEXT.review.other, description: '', detail: TEXT.review.otherDetail, available: true, selected: false, base: null });
  return out;
}

/** After "Other branch…": the two ways to compare with the branch chosen. */
export function branchChoices(branch: string): { readonly title: string; readonly choices: ReadonlyArray<{ readonly label: string; readonly mode: string; readonly branch: string }> } {
  return { title: TEXT.review.compareWith(branch), choices: [{ label: TEXT.review.mergeBase, mode: 'branch_merge_base', branch }, { label: TEXT.review.branchTip, mode: 'branch_tip', branch }] };
}

export interface DiffRow {
  readonly kind: 'removed' | 'added';
  readonly key: string;
  /** The key of the hunk it belongs to. */
  readonly hunk: string;
  /** Its line in the comparison; `null` for an added line. */
  readonly baseLine: number | null;
  /** Its line in the working copy; `null` for a removed line. */
  readonly modifiedLine: number | null;
  readonly text: string;
}

export interface HunkView {
  readonly key: string;
  /** From 1. */
  readonly index: number;
  readonly reviewed: boolean;
  /** "Hunk 2 of src/a.ts, lines 10–12" */
  readonly label: string;
  /** "lines 10–12", "deletion after line 9" */
  readonly where: string;
  readonly baseStart: number;
  readonly modifiedStart: number;
  readonly removed: number;
  readonly added: number;
  /** The removed lines, then the added ones. */
  readonly rows: ReadonlyArray<DiffRow>;
  /** Accept marks it reviewed; on a reviewed hunk it takes the mark away. `reviewed` is what it would become. */
  readonly accept: { readonly label: string; readonly reviewed: boolean };
  readonly reject: { readonly label: string };
  /** As the daemon sent it, for `acceptParams`. */
  readonly hunk: Hunk;
}

export interface FileDiff {
  readonly path: string;
  /** False for a file that is not shown as text: binary, too large, a link. */
  readonly shown: boolean;
  readonly why: string;
  readonly hunks: ReadonlyArray<HunkView>;
  readonly added: number;
  readonly removed: number;
  readonly reviewed: number;
  /** Said in place of the hunks: "Not shown: a binary file.", "New file". */
  readonly note: string | null;
}

interface HunksResult {
  readonly path: string;
  readonly shown: boolean;
  readonly why?: string | null;
  readonly hunks: ReadonlyArray<Hunk>;
  readonly before?: { readonly exists: boolean; readonly kind: string };
  readonly now?: { readonly exists: boolean; readonly kind: string };
}

/** The result of `workspace.hunks` as rows. `marks`: the keys marked reviewed now (the store's), when they are newer than the result. */
export function fileDiff(result: HunksResult, marks?: Iterable<string>): FileDiff {
  const marked = marks !== undefined ? new Set(marks) : undefined;
  const t = TEXT.review;
  const hunks = result.hunks.map((hunk, at): HunkView => {
    const reviewed = marked !== undefined ? marked.has(hunk.key) : hunk.reviewed;
    const where = hunk.modified_lines.length ? t.lines(hunk.modified_start, hunk.modified_start + hunk.modified_lines.length - 1) : t.deletionAfter(hunk.modified_start);
    const rows: DiffRow[] = [
      ...hunk.base_lines.map((text, i): DiffRow => ({ kind: 'removed', key: `${hunk.key}:-${i}`, hunk: hunk.key, baseLine: hunk.base_start + i, modifiedLine: null, text })),
      ...hunk.modified_lines.map((text, i): DiffRow => ({ kind: 'added', key: `${hunk.key}:+${i}`, hunk: hunk.key, baseLine: null, modifiedLine: hunk.modified_start + i, text })),
    ];
    return {
      key: hunk.key, index: at + 1, reviewed, label: t.hunkOf(at + 1, result.path, where), where, baseStart: hunk.base_start, modifiedStart: hunk.modified_start,
      removed: hunk.base_lines.length, added: hunk.modified_lines.length, rows,
      accept: { label: reviewed ? t.unmark(at + 1) : t.accept(at + 1), reviewed: !reviewed }, reject: { label: t.reject(at + 1) }, hunk,
    };
  });
  let note: string | null = null;
  if (!result.shown) note = PHONE_ONLY.notShown(result.why ?? '');
  else if (result.before && !result.before.exists && result.now?.exists) note = PHONE_ONLY.newFile;
  else if (result.now && !result.now.exists && result.before?.exists) note = PHONE_ONLY.deletedFile;
  return {
    path: result.path, shown: result.shown, why: result.why ?? '', hunks, added: hunks.reduce((n, h) => n + h.added, 0), removed: hunks.reduce((n, h) => n + h.removed, 0),
    reviewed: hunks.filter(h => h.reviewed).length, note,
  };
}

/** The key of a hunk: the function the daemon (`hunk_key`) and VS Code (`hunkHash`) use, over UTF-16 code units. */
export function hunkKey(path: string, baseLines: ReadonlyArray<string>, modifiedLines: ReadonlyArray<string>): string {
  const text = `${path}\u0000${baseLines.join('\n')}\u0000${modifiedLines.join('\n')}`;
  let h1 = 0x811c9dc5, h2 = 0x01000193;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 16777619);
    h2 = Math.imul(h2 ^ c, 2246822519);
  }
  return (h1 >>> 0).toString(16).padStart(8, '0') + (h2 >>> 0).toString(16).padStart(8, '0');
}

const BREAK_AFTER = /[\s,;:)\]}>/\\.\-_=&|+]/;

/**
 * A long line in pieces of at most `width` characters, for a screen too narrow for it. It breaks
 * after a space or a mark where one is near the end of the piece, else where the width ends.
 * Nothing is added or dropped: the pieces joined are the line.
 */
export function splitLine(text: string, width: number): ReadonlyArray<string> {
  const max = Math.max(1, Math.floor(width));
  if (text.length <= max) return [text];
  const out: string[] = [];
  let at = 0;
  while (text.length - at > max) {
    let cut = at + max;
    for (let i = cut; i > at + Math.floor(max / 2); i--) {
      if (BREAK_AFTER.test(text[i - 1] as string)) { cut = i; break; }
    }
    // Never between the two halves of one character.
    const code = text.charCodeAt(cut - 1);
    if (code >= 0xd800 && code <= 0xdbff && cut - 1 > at) cut--;
    out.push(text.slice(at, cut));
    at = cut;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}

/** Where an edit chip opens: the first hunk not yet reviewed, else the first. */
export function editTarget(diff: FileDiff): { readonly hunk: string; readonly line: number } | null {
  const hunk = diff.hunks.find(h => !h.reviewed) ?? diff.hunks[0];
  return hunk === undefined ? null : { hunk: hunk.key, line: Math.max(1, hunk.modifiedStart) };
}

/** The parameters of `review.accept` for a hunk; the caller adds `run_id`. `anchor`: the line a removal sits at, as shown. */
export function acceptParams(path: string, hunk: Hunk, anchor?: string): { path: string; key: string; modified_start: number; modified_lines: ReadonlyArray<string>; base_lines: ReadonlyArray<string>; anchor?: string } {
  return { path, key: hunk.key, modified_start: hunk.modified_start, modified_lines: hunk.modified_lines, base_lines: hunk.base_lines, ...(anchor !== undefined ? { anchor } : {}) };
}
