// One agent's conversation as the rows a phone lists, built from the daemon's events.
//
// It shows what VS Code's chat shows (extension/media/conversation.js), in the same order: your
// prompts, the agent's replies, tool calls as one-line rows that fold when several follow each
// other, edit chips, permission cards, errors, native children under the tool call that started
// them, and a quiet footer at the end of each turn. test/conversation-parity.test.ts loads the
// real conversation.js and compares the two, row by row, after every event.
//
//   create({ rootId })          an empty conversation
//   setRun(c, run, children)    what the state says about the run now (status, what it asks)
//   append(c, event)            the conversation after one event, and which rows changed
//   rowsOf(c)                   the rows, as one array
//
// Everything is pure: `append` returns a new conversation and leaves the old one as it was. The
// rows are a flat list with a depth, ready for a list that builds only what is visible. One event
// changes a few rows; the rest of the list is shared with the conversation before.

import { describe, parseInput, SPAWN_TOOLS } from './describe.ts';
import type { ToolDescription } from './describe.ts';
import { mapGet, mapSet, pmap, pvec, vecArray, vecGet, vecPush, vecSet, vecSplice } from './persistent.ts';
import type { PMap, PVec } from './persistent.ts';
import { parse } from './markdown.ts';
import { plain } from './plain.ts';
import type { Block } from './markdown.ts';
import { basename, compact, duration, statusText, TEXT } from './text.ts';
import { record } from './types.ts';
import type { DaemonEvent, Run } from './types.ts';

// ------------------------------------------------------------------ rows

interface RowBase {
  /** Stays the same for as long as the row exists. */
  readonly key: string;
  /** 0 for what a turn holds itself; one more for each fold, tool call or child it is under. */
  readonly depth: number;
  /** Which turn of this conversation, counted from 0. */
  readonly turn: number;
  /** The turn's number as the daemon counts it; 0 for what came before the first turn. */
  readonly turnNumber: number | string;
  /** The run the row belongs to: the agent itself, or one of its native children. */
  readonly run: string;
  /** The key of what holds it: a turn, a fold, a tool call (for its children) or a child. */
  readonly parent: string;
  /** The event that made it. */
  readonly seq: number;
}

/** Your prompt. `sent` is 'queued' or 'sending' for a message the daemon has not started yet. */
export interface UserRow extends RowBase {
  readonly kind: 'user';
  readonly label: string;
  readonly text: string;
  readonly sent: 'sent' | 'queued' | 'sending' | 'failed';
  /** The request that carried it, when it was sent from a phone. */
  readonly requestId: string | null;
}

/** What the agent said (Markdown), or what a program printed (plain text). */
export interface MessageRow extends RowBase {
  readonly kind: 'message';
  /** 'assistant', 'stdout', 'stderr', 'user', or whatever the harness called it. */
  readonly role: string;
  /** "Agent", "Sub-agent", or the role. */
  readonly label: string;
  readonly text: string;
  /** True for the agent's replies: `text` is Markdown. */
  readonly markdown: boolean;
}

/** Reasoning or a plan: closed until it is opened. */
export interface ThinkingRow extends RowBase {
  readonly kind: 'thinking';
  readonly role: 'reasoning' | 'plan';
  readonly label: string;
  readonly icon: string;
  /** Markdown. */
  readonly text: string;
}

/** Several tool calls in a row, folded into one line: "6 steps · Read · Searched · Edited 2×". */
export interface StepsRow extends RowBase {
  readonly kind: 'steps';
  readonly icon: string;
  readonly count: number;
  /** "6 steps" */
  readonly label: string;
  readonly verbs: ReadonlyArray<{ readonly verb: string; readonly count: number }>;
  /** "Read · Searched · Edited 2×" */
  readonly summary: string;
  readonly failed: number;
  /** "1 failed", or empty. */
  readonly failedText: string;
  /** "Read, Searched, Edited 2×" */
  readonly tooltip: string;
}

export type ToolResult =
  | { readonly state: 'running' }
  | { readonly state: 'ok' }
  | { readonly state: 'failed'; readonly text: string }
  | { readonly state: 'changed'; readonly added: number; readonly removed: number; readonly addedText: string; readonly removedText: string };

export interface ToolRow extends RowBase {
  readonly kind: 'tool';
  readonly id: string;
  /** The harness's name for the tool: "Bash", "Edit". */
  readonly name: string;
  readonly icon: string;
  /** "Ran", or "Run" while it runs. */
  readonly verb: string;
  /** Short: a file's name, the first line of a command. */
  readonly target: string;
  /** The target is code. */
  readonly code: boolean;
  /** In full, with the tool's name: the whole path or command. VS Code's tooltip. */
  readonly full: string;
  readonly result: ToolResult;
  /** The fold it is in, when it is in one. */
  readonly group: string | null;
  /** For `toolDetail`: what the call was given and what it returned. */
  readonly input: unknown;
  readonly output: unknown;
  readonly isError: boolean;
  readonly reported: string | null;
  readonly summary: string;
}

/** Files the agent changed: each opens the review at the hunk. */
export interface EditRow extends RowBase {
  readonly kind: 'edit';
  readonly icon: string;
  readonly files: ReadonlyArray<{ readonly name: string; readonly path: string; readonly tooltip: string }>;
  /** 'reported', or how the daemon knows: 'tool-input'. */
  readonly confidence: string;
  /** The fold it sits under, when it follows tool calls. */
  readonly group: string | null;
}

export interface PermissionRow extends RowBase {
  readonly kind: 'permission';
  readonly requestId: unknown;
  readonly tool: unknown;
  readonly input: unknown;
  /** pending: waiting for you. asked: asked before, and no longer open. */
  readonly state: 'pending' | 'allowed' | 'denied' | 'asked';
  readonly icon: string;
  /** "Allow Create perm.txt?", "Allowed · Create perm.txt" */
  readonly text: string;
  /** The whole path or command. */
  readonly full: string;
  /** The first lines of what would be written or run, while it is pending. */
  readonly preview: string | null;
  /** Who answered: "the Mac", "phone:Bilal's iPhone". VS Code does not show it. */
  readonly by: string | null;
}

export interface ErrorRow extends RowBase {
  readonly kind: 'error';
  readonly icon: string;
  /** auth, rate_limit, quota, network, other. */
  readonly class: string;
  readonly title: string;
  readonly message: string;
  /** Offers "Sign in again". */
  readonly signIn: boolean;
}

/** A native child: its rows follow, one deeper. */
export interface ChildRow extends RowBase {
  readonly kind: 'child';
  readonly icon: string;
  readonly childRun: string;
  readonly title: string;
  readonly status: string;
  readonly statusText: string;
  /** What it reported using, beside its title: "20k reported tokens"; empty until it reports. */
  readonly usage: string;
  readonly tooltip: string;
}

/** A quiet line: how a run ended outside a turn, or an event the chat has no picture for. */
export interface NoteRow extends RowBase {
  readonly kind: 'note';
  readonly text: string;
  readonly status: string | null;
  readonly icon: string | null;
  readonly tooltip: string | null;
}

/** The end of a turn: how it ended, how long it took, tokens and cost. */
export interface FooterRow extends RowBase {
  readonly kind: 'footer';
  readonly state: 'ok' | 'stopped' | 'fail' | null;
  readonly icon: string | null;
  /** "Done", "Stopped", "Failed: the reason" */
  readonly text: string;
  readonly tooltip: string | null;
  /** "48s" */
  readonly duration: string;
  /** "20k tokens · $0.04" */
  readonly usage: string;
  /** "18,423 in · 1,204 out · 9,321 cached · $0.0412" */
  readonly usageDetail: string;
}

export type Row = UserRow | MessageRow | ThinkingRow | StepsRow | ToolRow | EditRow | PermissionRow | ErrorRow | ChildRow | NoteRow | FooterRow;

// ------------------------------------------------------------------ the conversation

interface TurnState {
  readonly n: number | string;
  readonly started: number | undefined;
  readonly ended: boolean;
  readonly hadError: boolean;
  readonly foot: boolean;
}

interface ToolState {
  readonly key: string;
  readonly id: string | undefined;
  readonly name: string;
  readonly run: string;
  readonly group: string | undefined;
  readonly summary: string;
  readonly status: string | undefined;
  readonly input: unknown;
  readonly output: unknown;
  readonly reported: unknown;
  readonly isError: unknown;
  readonly failedCounted: boolean;
  readonly verb: string;
  /** Child blocks directly under it. */
  readonly kids: number;
  /** Near where its row is. */
  readonly at: number;
}

interface GroupState {
  readonly key: string;
  /** What holds the fold. */
  readonly parent: string;
  readonly tools: ReadonlyArray<string>;
  readonly edits: number;
  readonly failed: number;
  readonly at: number;
}

interface ChildInfo {
  readonly id?: string;
  readonly title?: string | null;
  readonly status?: string | null;
  readonly native_id?: string | null;
  readonly parent?: string | null;
  readonly evidence?: unknown;
  readonly confidence?: string | null;
}

interface BlockState {
  readonly run: string;
  readonly key: string;
  /** Under a tool call (its id, or true), moved by the daemon ('reparented'), or where it first appeared. */
  readonly nested: string | true | undefined;
  readonly at: number;
}

interface CardState {
  readonly key: string;
  readonly answer: 'allowed' | 'denied' | undefined;
  readonly by: string | null;
  readonly at: number;
}

export interface Conversation {
  readonly rootId: string;
  /** The Mac's home folder, when the app knows it: long paths in replies start with "~". */
  readonly home: string;
  readonly rows: PVec<Row>;
  /** True while the agent works: the app shows `working.label` under the rows. */
  readonly working: { readonly shown: boolean; readonly label: string };
  /** Said above the rows when older history is gone. */
  readonly banner: string | null;
  /** The run's status as `setRun` last gave it. */
  readonly status: string | undefined;
  /** The request the run waits on, as `setRun` last gave it. */
  readonly attention: unknown;
  /** Requests sent from a phone that became a turn: request id to the turn's place. */
  readonly landed: PMap<number>;

  // What follows is the builder's own bookkeeping.
  readonly active: boolean;
  readonly stopping: boolean;
  readonly seen: PMap<ReadonlyArray<number>>;
  readonly family: PMap<true>;
  readonly turns: ReadonlyArray<TurnState>;
  readonly tools: PMap<ToolState>;
  readonly toolsOf: PMap<PVec<string>>;
  /** Tool calls of the agent itself that still show as running. */
  readonly open: ReadonlyArray<string>;
  readonly groups: PMap<GroupState>;
  readonly blocks: PMap<BlockState>;
  readonly blockOrder: ReadonlyArray<string>;
  readonly childInfo: PMap<ChildInfo>;
  /** What each native child reported using: its label and the tooltip's line. */
  readonly childUsage: PMap<{ readonly label: string; readonly detail: string }>;
  readonly cards: PMap<CardState>;
  readonly cardRequests: PMap<unknown>;
  /** A request from a phone that was just accepted: the next turn is its message. */
  readonly arriving: string | null;
}

export interface Appended {
  readonly conversation: Conversation;
  /** Places, in the new list, of the rows that are new or different. In rising order. */
  readonly changed: ReadonlyArray<number>;
  /** The first place from which rows now sit at another place than before, or -1. */
  readonly movedFrom: number;
}

export interface ConversationOptions {
  readonly rootId: string;
  readonly home?: string;
}

export function create(options: ConversationOptions): Conversation {
  return {
    rootId: options.rootId, home: options.home ?? '', rows: pvec(), working: { shown: false, label: TEXT.conversation.working }, banner: null, status: undefined, attention: undefined,
    landed: pmap(), active: false, stopping: false, seen: pmap(), family: mapSet(pmap<true>(), options.rootId, true), turns: [], tools: pmap(), toolsOf: pmap(), open: [], groups: pmap(),
    blocks: pmap(), blockOrder: [], childInfo: pmap(), childUsage: pmap(), cards: pmap(), cardRequests: pmap(), arriving: null,
  };
}

/** The rows as one array. Built once for each version of the conversation. */
export function rowsOf(conversation: Conversation): ReadonlyArray<Row> {
  return vecArray(conversation.rows);
}

export function rowAt(conversation: Conversation, index: number): Row | undefined {
  return vecGet(conversation.rows, index);
}

const visible = new WeakMap<PVec<Row>, WeakMap<ReadonlySet<string>, ReadonlyArray<Row>>>();
const NOTHING_TOGGLED: ReadonlySet<string> = new Set();

/**
 * The rows a person sees. As in VS Code, a fold of steps is closed until it is opened and a child
 * is open until it is closed: `toggled` holds the keys of the folds opened and the children closed.
 */
export function visibleRows(conversation: Conversation, toggled: ReadonlySet<string> = NOTHING_TOGGLED): ReadonlyArray<Row> {
  let byToggle = visible.get(conversation.rows);
  if (byToggle === undefined) visible.set(conversation.rows, byToggle = new WeakMap());
  const known = byToggle.get(toggled);
  if (known !== undefined) return known;
  const out: Row[] = [];
  let hideDeeperThan = -1;
  for (const row of vecArray(conversation.rows)) {
    if (hideDeeperThan >= 0) {
      if (row.depth > hideDeeperThan) continue;
      hideDeeperThan = -1;
    }
    out.push(row);
    if ((row.kind === 'steps' && !toggled.has(row.key)) || (row.kind === 'child' && toggled.has(row.key))) hideDeeperThan = row.depth;
  }
  byToggle.set(toggled, out);
  return out;
}

// ------------------------------------------------------------------ one change

type Writable<T> = { -readonly [K in keyof T]: T[K] };

interface Work {
  c: Writable<Conversation>;
  /** Rows made or changed, with a place near each. */
  touched: Map<string, number>;
  moved: number;
}

function begin(conversation: Conversation): Work {
  return { c: { ...conversation }, touched: new Map(), moved: -1 };
}

function finish(w: Work, before: Conversation): Appended {
  if (!w.touched.size && w.moved < 0 && same(w.c, before)) return { conversation: before, changed: [], movedFrom: -1 };
  const changed: number[] = [];
  for (const [key, near] of w.touched) {
    const at = locate(w.c.rows, key, near);
    if (at >= 0) changed.push(at);
  }
  changed.sort((a, b) => a - b);
  return { conversation: w.c, changed, movedFrom: w.moved };
}

function same(a: Conversation, b: Conversation): boolean {
  for (const key of Object.keys(a) as Array<keyof Conversation>) if (a[key] !== b[key]) return false;
  return true;
}

/** Where the row with this key is, looking outward from where it was last seen. */
function locate(rows: PVec<Row>, key: string, near: number): number {
  const n = rows.length;
  if (!n) return -1;
  let lo = Math.min(Math.max(near, 0), n - 1), hi = lo + 1;
  while (lo >= 0 || hi < n) {
    if (lo >= 0) {
      if ((vecGet(rows, lo) as Row).key === key) return lo;
      lo--;
    }
    if (hi < n) {
      if ((vecGet(rows, hi) as Row).key === key) return hi;
      hi++;
    }
  }
  return -1;
}

function put(w: Work, index: number, rows: ReadonlyArray<Row>): void {
  if (!rows.length) return;
  if (index < w.c.rows.length && (w.moved < 0 || index < w.moved)) w.moved = index;
  w.c.rows = rows.length === 1 && index === w.c.rows.length ? vecPush(w.c.rows, rows[0] as Row) : vecSplice(w.c.rows, index, 0, rows);
  rows.forEach((row, i) => w.touched.set(row.key, index + i));
}

function take(w: Work, index: number, count: number): Row[] {
  const out: Row[] = [];
  for (let i = 0; i < count; i++) out.push(vecGet(w.c.rows, index + i) as Row);
  if (w.moved < 0 || index < w.moved) w.moved = index;
  w.c.rows = vecSplice(w.c.rows, index, count, []);
  for (const row of out) w.touched.delete(row.key);
  return out;
}

function set(w: Work, index: number, row: Row): void {
  const was = vecGet(w.c.rows, index);
  if (was !== undefined && sameRow(was, row)) return;
  w.c.rows = vecSet(w.c.rows, index, row);
  w.touched.set(row.key, index);
}

/** Rows are small and flat: equal when every field is, looking one level into lists and results. */
function sameRow(a: Row, b: Row): boolean {
  if (a === b) return true;
  const ra = a as unknown as Record<string, unknown>, rb = b as unknown as Record<string, unknown>;
  const keys = Object.keys(ra);
  if (keys.length !== Object.keys(rb).length) return false;
  for (const k of keys) {
    const x = ra[k], y = rb[k];
    if (x === y) continue;
    if (x === null || y === null || typeof x !== 'object' || typeof y !== 'object') return false;
    if (k === 'input' || k === 'output') return false;
    if (JSON.stringify(x) !== JSON.stringify(y)) return false;
  }
  return true;
}

// ------------------------------------------------------------------ where things are

const turnKey = (ordinal: number): string => `turn:${ordinal}`;
/** A run's id holds no slash, so the two parts cannot be taken for each other. */
const toolKey = (run: string, id: string): string => `tool:${run}/${id}`;
const childKey = (run: string): string => `child:${run}`;

/** What holds the thing with this key: a fold's holder, a tool call's, a child's. */
function holderOf(c: Conversation, key: string, rows: PVec<Row>): string | undefined {
  if (key.startsWith('steps:')) return mapGet(c.groups, key)?.parent;
  if (key.startsWith('tool:')) {
    const t = mapGet(c.tools, key);
    if (t === undefined) return undefined;
    if (t.group !== undefined) return t.group;
    const at = locate(rows, key, t.at);
    return at < 0 ? undefined : (vecGet(rows, at) as Row).parent;
  }
  if (key.startsWith('child:')) {
    const b = mapGet(c.blocks, key.slice(6));
    if (b === undefined) return undefined;
    const at = locate(rows, key, b.at);
    return at < 0 ? undefined : (vecGet(rows, at) as Row).parent;
  }
  return undefined;
}

/** True when the row is somewhere under the thing with this key. */
function under(c: Conversation, row: Row, owner: string): boolean {
  let key: string | undefined = row.parent;
  for (let i = 0; key !== undefined && i < 64; i++) {
    if (key === owner) return true;
    key = holderOf(c, key, c.rows);
  }
  return false;
}

/** One past the last row under the row at `index`. */
function endOfSubtree(c: Conversation, index: number): number {
  const rows = c.rows;
  const top = vecGet(rows, index) as Row;
  // Most often the thing is the last in the list: look there before walking through it.
  for (const back of [1, 2]) {
    const last = vecGet(rows, rows.length - back);
    if (last === undefined || rows.length - back <= index) break;
    if (last.kind === 'footer') continue;
    if (under(c, last, top.key)) return rows.length - back + 1;
    break;
  }
  let end = index + 1;
  while (end < rows.length && (vecGet(rows, end) as Row).depth > top.depth) end++;
  return end;
}

interface Place {
  /** The key of what holds the rows. */
  readonly owner: string;
  readonly depth: number;
  readonly turn: number;
  readonly turnNumber: number | string;
  /** Where a new row goes: after everything it holds. */
  readonly end: number;
  /** The last row it holds, however deep, or undefined when it holds nothing. */
  readonly last: Row | undefined;
}

function ensureTurn(w: Work, event?: DaemonEvent): number {
  if (!w.c.turns.length) newTurn(w, { n: 0, prompt: '' }, event, true);
  return w.c.turns.length - 1;
}

function newTurn(w: Work, t: { n: number | string; prompt: unknown }, event: DaemonEvent | undefined, implicit: boolean): void {
  const ordinal = w.c.turns.length;
  const started = implicit || event === undefined ? undefined : eventTime(event);
  w.c.turns = [...w.c.turns, { n: t.n, started, ended: false, hadError: false, foot: false }];
  if (t.prompt) {
    const requestId = w.c.arriving;
    const row: UserRow = {
      kind: 'user', key: requestId !== null ? `sent:${requestId}` : `${turnKey(ordinal)}:user`, depth: 0, turn: ordinal, turnNumber: t.n, run: w.c.rootId, parent: turnKey(ordinal), seq: event?.seq ?? 0,
      label: TEXT.conversation.you, text: String(t.prompt), sent: 'sent', requestId,
    };
    put(w, w.c.rows.length, [row]);
  }
  if (w.c.arriving !== null) {
    w.c.landed = mapSet(w.c.landed, w.c.arriving, ordinal);
    w.c.arriving = null;
  }
}

const eventTime = (event: DaemonEvent): number | undefined => {
  const e = event as DaemonEvent & { ts_ms?: number };
  return e.ts_ms || e.ts || undefined;
};

/** The body of the agent's current turn. */
function rootPlace(w: Work, event?: DaemonEvent): Place {
  const ordinal = ensureTurn(w, event);
  const turn = w.c.turns[ordinal] as TurnState;
  const end = w.c.rows.length - (turn.foot ? 1 : 0);
  const last = vecGet(w.c.rows, end - 1);
  const held = last !== undefined && last.turn === ordinal && last.kind !== 'user' ? last : undefined;
  return { owner: turnKey(ordinal), depth: 0, turn: ordinal, turnNumber: turn.n, end, last: held };
}

/** What a child's block holds. */
function childPlace(w: Work, run: string, event?: DaemonEvent): Place {
  const block = childBlock(w, run, undefined, event);
  const at = locate(w.c.rows, block.key, block.at);
  const top = vecGet(w.c.rows, at) as Row;
  const end = endOfSubtree(w.c, at);
  return { owner: block.key, depth: top.depth + 1, turn: top.turn, turnNumber: top.turnNumber, end, last: end - 1 > at ? vecGet(w.c.rows, end - 1) : undefined };
}

function placeFor(w: Work, event: DaemonEvent): Place {
  return !event.run_id || event.run_id === w.c.rootId ? rootPlace(w, event) : childPlace(w, event.run_id, event);
}

/** The fold at the end of a place, when what it holds ends with one. */
function lastGroup(c: Conversation, place: Place): GroupState | undefined {
  let row = place.last;
  if (row === undefined) return undefined;
  let key = row.key, parent: string | undefined = row.parent;
  for (let i = 0; parent !== undefined && i < 64; i++) {
    if (parent === place.owner) return key.startsWith('steps:') ? mapGet(c.groups, key) : undefined;
    key = parent;
    parent = holderOf(c, key, c.rows);
  }
  row = undefined;
  return undefined;
}

function base(place: Place, key: string, run: string, seq: number, parent = place.owner, depth = place.depth): RowBase {
  return { key, depth, turn: place.turn, turnNumber: place.turnNumber, run, parent, seq };
}

// ------------------------------------------------------------------ tools and folds

function toolRow(b: RowBase, t: ToolState, c: Conversation): ToolRow {
  const d = describe(t.name, t.input, t.summary);
  const done = !!t.status && !/running|started|inProgress|in_progress/.test(t.status);
  const verb = !done && d.pending && t.status !== undefined ? d.pending : d.verb;
  // The tool's own name follows in brackets, unless it is an internal one (mcp__server__tool, AC-245).
  const full = [d.full, t.name !== d.verb && !/^mcp__/.test(t.name) ? `(${t.name})` : ''].filter(Boolean).join(' ');
  return {
    ...b, kind: 'tool', id: t.id ?? '', name: t.name, icon: d.icon, verb, target: d.target || '', code: !!d.code, full,
    result: resultOf(t, d, c), group: t.group ?? null,
    input: t.input, output: t.output, isError: !!t.isError, reported: typeof t.reported === 'string' ? t.reported : t.reported === undefined || t.reported === null ? null : String(t.reported), summary: t.summary,
  };
}

const failedOf = (t: ToolState): boolean => !!(t.isError || /failed|error|declined/.test(String(t.reported || t.status || '')));

function resultOf(t: ToolState, d: ToolDescription, c: Conversation): ToolResult {
  const st = String(t.reported || t.status || '');
  const running = !st || /running|started|inProgress|in_progress/.test(st);
  if (failedOf(t)) {
    const exit = /exit (\d+)/.exec(t.summary || '');
    return { state: 'failed', text: exit && exit[1] !== '0' ? TEXT.conversation.exit(exit[1] as string) : TEXT.conversation.failedResult };
  }
  if (running && !(!c.active && t.run === c.rootId)) return { state: 'running' };
  if (d.added || d.removed) return { state: 'changed', added: d.added || 0, removed: d.removed || 0, addedText: TEXT.conversation.added(d.added || 0), removedText: TEXT.conversation.removed(d.removed || 0) };
  return { state: 'ok' };
}

function stepsRow(b: RowBase, g: GroupState, c: Conversation): StepsRow {
  const counts = new Map<string, number>();
  for (const key of g.tools) {
    const t = mapGet(c.tools, key);
    if (t !== undefined) counts.set(t.verb || t.name, (counts.get(t.verb || t.name) || 0) + 1);
  }
  const verbs = [...counts].map(([verb, count]) => ({ verb, count }));
  const parts = verbs.map(v => (v.count > 1 ? TEXT.conversation.times(v.verb, v.count) : v.verb));
  return {
    ...b, kind: 'steps', icon: 'tools', count: g.tools.length, label: TEXT.conversation.steps(g.tools.length), verbs, summary: parts.join(' · '),
    failed: g.failed, failedText: g.failed ? TEXT.conversation.stepsFailed(g.failed) : '', tooltip: parts.join(', '),
  };
}

/** The row of a tool call, made when it is first heard of. */
function toolCard(w: Work, event: DaemonEvent, id: unknown, name: unknown): ToolState {
  const run = event.run_id || w.c.rootId;
  const given = typeof id === 'string' && id ? id : id ? String(id) : undefined;
  const key = toolKey(run, given || 'seq' + event.seq);
  const known = mapGet(w.c.tools, key);
  if (known !== undefined) return known;
  const place = placeFor(w, event);
  const called = typeof name === 'string' ? name : name ? String(name) : '';
  let state: ToolState = { key, id: given, name: called, run, group: undefined, summary: '', status: undefined, input: undefined, output: undefined, reported: undefined, isError: undefined, failedCounted: false, verb: '', kids: 0, at: place.end };
  if (SPAWN_TOOLS.test(called)) {
    put(w, place.end, [toolRow(base(place, key, run, event.seq), state, w.c)]);
  } else {
    const group = lastGroup(w.c, place);
    if (group !== undefined) {
      state = { ...state, group: group.key, at: place.end - group.edits };
      const folded = group.tools.length > 1;
      put(w, place.end - group.edits, [toolRow(base(place, key, run, event.seq, group.key, place.depth + 1), state, w.c)]);
      w.c.groups = mapSet(w.c.groups, group.key, { ...group, tools: [...group.tools, key] });
      if (!folded) fold(w, group.key, place, event.seq);
    } else {
      const gkey = `steps:${event.seq}`;
      state = { ...state, group: gkey };
      w.c.groups = mapSet(w.c.groups, gkey, { key: gkey, parent: place.owner, tools: [key], edits: 0, failed: 0, at: place.end });
      put(w, place.end, [toolRow(base(place, key, run, event.seq, gkey), state, w.c)]);
    }
  }
  w.c.tools = mapSet(w.c.tools, key, state);
  w.c.toolsOf = mapSet(w.c.toolsOf, run, vecPush(mapGet(w.c.toolsOf, run) || pvec<string>(), key));
  if (given !== undefined) adopt(w, state);
  return mapGet(w.c.tools, key) as ToolState;
}

/** A second tool call joined the first: the two fold under one line, one deeper. */
function fold(w: Work, gkey: string, place: Place, seq: number): void {
  const g = mapGet(w.c.groups, gkey) as GroupState;
  const first = mapGet(w.c.tools, g.tools[0] as string) as ToolState;
  const at = locate(w.c.rows, first.key, first.at);
  const top = vecGet(w.c.rows, at) as Row;
  // The first call and what is under it go one deeper; the second call is already there.
  let end = at + 1;
  while (end < w.c.rows.length && (vecGet(w.c.rows, end) as Row).depth > top.depth && (vecGet(w.c.rows, end) as Row).key !== g.tools[1]) end++;
  for (let i = at; i < end; i++) set(w, i, { ...(vecGet(w.c.rows, i) as Row), depth: (vecGet(w.c.rows, i) as Row).depth + 1 });
  put(w, at, [stepsRow({ key: gkey, depth: place.depth, turn: top.turn, turnNumber: top.turnNumber, run: top.run, parent: place.owner, seq }, g, w.c)]);
  w.c.groups = mapSet(w.c.groups, gkey, { ...g, at });
}

/** Brings a tool call's row, and its fold's line, up to date. */
function label(w: Work, key: string): void {
  let t = mapGet(w.c.tools, key) as ToolState;
  const d = describe(t.name, t.input, t.summary);
  t = { ...t, verb: d.verb };
  const at = locate(w.c.rows, key, t.at);
  if (at < 0) return;
  const was = vecGet(w.c.rows, at) as ToolRow;
  const failed = failedOf(t);
  if (t.group !== undefined && failed !== t.failedCounted) {
    const g = mapGet(w.c.groups, t.group) as GroupState;
    w.c.groups = mapSet(w.c.groups, g.key, { ...g, failed: g.failed + (failed ? 1 : -1) });
    t = { ...t, failedCounted: failed };
  }
  t = { ...t, at };
  w.c.tools = mapSet(w.c.tools, key, t);
  const row = toolRow(was, t, w.c);
  set(w, at, row);
  track(w, t, row);
  if (t.group !== undefined) summarize(w, t.group);
}

/** Remembers the agent's own calls that still show as running: the end of the turn settles them. */
function track(w: Work, t: ToolState, row: ToolRow): void {
  if (t.run !== w.c.rootId) return;
  const open = w.c.open.includes(t.key);
  if (row.result.state === 'running' && !open) w.c.open = [...w.c.open, t.key];
  else if (row.result.state !== 'running' && open) w.c.open = w.c.open.filter(k => k !== t.key);
}

function summarize(w: Work, gkey: string): void {
  const g = mapGet(w.c.groups, gkey) as GroupState;
  if (g.tools.length < 2) return;
  const at = locate(w.c.rows, gkey, g.at);
  if (at < 0) return;
  if (at !== g.at) w.c.groups = mapSet(w.c.groups, gkey, { ...g, at });
  set(w, at, stepsRow(vecGet(w.c.rows, at) as Row, g, w.c));
}

// ------------------------------------------------------------------ children

function childRow(b: RowBase, run: string, c: Conversation): ChildRow {
  const info = mapGet(c.childInfo, run) || {};
  const st = info.status || 'unknown';
  const used = mapGet(c.childUsage, run);
  // Its title, state and usage; how it was linked to its parent is the daemon's business (AC-245).
  return {
    ...b, kind: 'child', icon: 'type-hierarchy-sub', childRun: run, title: info.title || TEXT.conversation.subAgent, status: st, statusText: statusText(st), usage: used?.label ?? '',
    tooltip: [info.title, statusText(st), used?.detail].filter(Boolean).join('\n'),
  };
}

/** A child's block, made where VS Code puts it when it is first heard of. */
function childBlock(w: Work, run: string, hint: ChildInfo | undefined, event?: DaemonEvent): BlockState {
  const known = mapGet(w.c.blocks, run);
  if (known !== undefined) return known;
  const info: ChildInfo = hint || mapGet(w.c.childInfo, run) || { id: run, title: TEXT.conversation.subAgent };
  if (hint !== undefined) w.c.childInfo = mapSet(w.c.childInfo, run, { ...mapGet(w.c.childInfo, run), ...hint });
  const key = childKey(run);
  const parent = info.parent;
  const scope = parent && parent !== w.c.rootId ? parent : w.c.rootId;
  const evidence = String(info.evidence || '');
  let host: ToolState | undefined;
  const calls = mapGet(w.c.toolsOf, scope);
  if (calls !== undefined) {
    for (const k of vecArray(calls)) {
      const t = mapGet(w.c.tools, k) as ToolState;
      if (t.id && (t.id === info.native_id || evidence.includes(t.id))) host = t;
    }
    if (host === undefined) {
      const list = vecArray(calls);
      for (let i = list.length - 1; i >= 0; i--) {
        const t = mapGet(w.c.tools, list[i] as string) as ToolState;
        if (SPAWN_TOOLS.test(t.name) && !t.kids) { host = t; break; }
      }
    }
  }
  let block: BlockState;
  const seq = event?.seq ?? 0;
  if (host !== undefined) {
    const at = locate(w.c.rows, host.key, host.at);
    const top = vecGet(w.c.rows, at) as Row;
    const end = endOfSubtree(w.c, at);
    block = { run, key, nested: host.id || true, at: end };
    w.c.blocks = mapSet(w.c.blocks, run, block);
    put(w, end, [childRow({ key, depth: top.depth + 1, turn: top.turn, turnNumber: top.turnNumber, run, parent: host.key, seq }, run, w.c)]);
    w.c.tools = mapSet(w.c.tools, host.key, { ...host, kids: host.kids + 1, at });
  } else {
    const place = scope !== w.c.rootId && mapGet(w.c.blocks, scope) !== undefined ? childPlace(w, scope, event) : rootPlace(w, event);
    block = { run, key, nested: undefined, at: place.end };
    w.c.blocks = mapSet(w.c.blocks, run, block);
    put(w, place.end, [childRow(base(place, key, run, seq), run, w.c)]);
  }
  w.c.blockOrder = [...w.c.blockOrder, run];
  w.c.family = mapSet(w.c.family, run, true);
  return block;
}

function childHeader(w: Work, run: string): void {
  const block = mapGet(w.c.blocks, run);
  if (block === undefined) return;
  const at = locate(w.c.rows, block.key, block.at);
  if (at < 0) return;
  set(w, at, childRow(vecGet(w.c.rows, at) as Row, run, w.c));
}

/** A tool call that started children seen before it takes them under it. */
function adopt(w: Work, card: ToolState): void {
  const id = card.id as string;
  for (const run of w.c.blockOrder) {
    const block = mapGet(w.c.blocks, run) as BlockState;
    const info = mapGet(w.c.childInfo, run) || {};
    const sameParent = !info.parent || info.parent === card.run || (card.run === w.c.rootId && info.parent === w.c.rootId);
    if (block.nested || !sameParent) continue;
    if (!(info.native_id === id || String(info.evidence || '').split(' inside ')[0]?.split(/[\s()]+/).includes(id))) continue;
    const now = mapGet(w.c.tools, card.key) as ToolState;
    const at = locate(w.c.rows, card.key, now.at);
    move(w, run, { owner: card.key, under: at });
    w.c.tools = mapSet(w.c.tools, card.key, { ...(mapGet(w.c.tools, card.key) as ToolState), kids: now.kids + 1 });
    w.c.blocks = mapSet(w.c.blocks, run, { ...(mapGet(w.c.blocks, run) as BlockState), nested: id });
  }
}

/** Takes a child's block, with everything under it, to the end of what another row holds. */
function move(w: Work, run: string, to: { owner: string; under: number }): void {
  const block = mapGet(w.c.blocks, run) as BlockState;
  const from = locate(w.c.rows, block.key, block.at);
  const count = endOfSubtree(w.c, from) - from;
  const was = vecGet(w.c.rows, from) as Row;
  // The tool call it leaves has one child fewer.
  if (was.parent.startsWith('tool:')) {
    const left = mapGet(w.c.tools, was.parent);
    if (left !== undefined) w.c.tools = mapSet(w.c.tools, left.key, { ...left, kids: Math.max(0, left.kids - 1) });
  }
  const destination = vecGet(w.c.rows, to.under) as Row;
  const rows = take(w, from, count);
  const under = locate(w.c.rows, destination.key, to.under);
  const top = vecGet(w.c.rows, under) as Row;
  const end = endOfSubtree(w.c, under);
  const by = top.depth + 1 - was.depth;
  put(w, end, rows.map((row, i) => ({ ...row, depth: row.depth + by, turn: top.turn, turnNumber: top.turnNumber, parent: i === 0 ? to.owner : row.parent })));
  w.c.blocks = mapSet(w.c.blocks, run, { ...block, at: end });
}

// ------------------------------------------------------------------ permission cards, footers

function cardRow(b: RowBase, requestId: unknown, tool: unknown, input: unknown, card: { answer: 'allowed' | 'denied' | undefined; by: string | null }, c: Conversation): PermissionRow {
  const pending = !card.answer && c.attention === requestId;
  const d = describe(tool, input);
  const what = `${d.pending || d.verb} ${d.target || ''}`.trim();
  const t = TEXT.conversation;
  let preview: string | null = null;
  if (pending) {
    const parsed = parseInput(input);
    const shown = parsed['command'] || parsed['content'] || parsed['new_string'];
    if (shown) preview = String(shown).split('\n').slice(0, 8).join('\n');
  }
  return {
    ...b, kind: 'permission', requestId, tool, input, state: pending ? 'pending' : card.answer || 'asked',
    icon: pending ? 'shield' : card.answer === 'allowed' ? 'check' : card.answer === 'denied' ? 'circle-slash' : 'shield',
    text: pending ? t.allow(what) : card.answer === 'allowed' ? t.allowed(what) : card.answer === 'denied' ? t.denied(what) : t.asked(what),
    full: String(d.full || tool), preview, by: card.by,
  };
}

function renderCard(w: Work, requestId: unknown): void {
  const card = mapGet(w.c.cards, String(requestId));
  if (card === undefined) return;
  const at = locate(w.c.rows, card.key, card.at);
  if (at < 0) return;
  const was = vecGet(w.c.rows, at) as PermissionRow;
  set(w, at, cardRow(was, was.requestId, was.tool, was.input, card, w.c));
}

const EMPTY_FOOT = { state: null, icon: null, text: '', tooltip: null, duration: '', usage: '', usageDetail: '' } as const;

/** The footer of the agent's current turn, shown from now on. */
function foot(w: Work, ordinal: number, change: Partial<FooterRow>): void {
  const turn = w.c.turns[ordinal] as TurnState;
  const key = `${turnKey(ordinal)}:foot`;
  if (turn.foot) {
    const at = locate(w.c.rows, key, w.c.rows.length - 1);
    set(w, at, { ...(vecGet(w.c.rows, at) as FooterRow), ...change });
    return;
  }
  w.c.turns = w.c.turns.map((t, i) => (i === ordinal ? { ...t, foot: true } : t));
  put(w, w.c.rows.length, [{ kind: 'footer', key, depth: 0, turn: ordinal, turnNumber: turn.n, run: w.c.rootId, parent: turnKey(ordinal), seq: 0, ...EMPTY_FOOT, ...change }]);
}

function turnChange(w: Work, ordinal: number, change: Partial<TurnState>): void {
  w.c.turns = w.c.turns.map((t, i) => (i === ordinal ? { ...t, ...change } : t));
}

function working(w: Work, label?: string): void {
  const next = { shown: w.c.active, label: label ?? w.c.working.label };
  if (next.shown !== w.c.working.shown || next.label !== w.c.working.label) w.c.working = next;
}

// ------------------------------------------------------------------ what was seen

function seenBefore(w: Work, seq: number): boolean {
  const page = String(Math.floor(seq / 1024));
  const bits = mapGet(w.c.seen, page);
  const word = (seq % 1024) >> 5, bit = 1 << (seq % 32);
  if (bits !== undefined && ((bits[word] as number) & bit) !== 0) return true;
  const next = bits !== undefined ? bits.slice() : new Array<number>(32).fill(0);
  next[word] = (next[word] as number) | bit;
  w.c.seen = mapSet(w.c.seen, page, next);
  return false;
}

/** Kinds that say nothing in a chat, and kinds the chat has a picture for; any other is a quiet line. */
const QUIET = new Set(['session', 'task_created', 'reattached', 'interrupt_requested', 'workspace_removed', 'background_notice', 'daemon_stopping', 'status', 'usage', 'push']);
const KNOWN = new Set(['turn_started', 'output', 'tool', 'tool_result', 'file_activity', 'permission', 'permission_answered', 'error', 'child', 'child_reparented', 'turn_done', 'retention', 'raw_unparsed', 'remote_command']);

/** True when the event is this agent's or one of its native children's: what VS Code's feed passes on. */
export function belongs(conversation: Conversation, event: DaemonEvent): boolean {
  return !!event.run_id && mapGet(conversation.family, event.run_id) === true;
}

/** The conversation after one event. An event seen before, or of another agent, changes nothing. */
export function append(conversation: Conversation, event: DaemonEvent): Appended {
  if (!belongs(conversation, event)) return { conversation, changed: [], movedFrom: -1 };
  const w = begin(conversation);
  if (seenBefore(w, event.seq)) return { conversation, changed: [], movedFrom: -1 };
  add(w, event);
  return finish(w, conversation);
}

/** Several events at once, as one change. */
export function appendAll(conversation: Conversation, events: Iterable<DaemonEvent>): Appended {
  const w = begin(conversation);
  for (const event of events) {
    if (!belongs(w.c, event) || seenBefore(w, event.seq)) continue;
    add(w, event);
  }
  return finish(w, conversation);
}

/** A conversation from a run's history. */
export function build(options: ConversationOptions, events: Iterable<DaemonEvent>, run?: Run, children?: ReadonlyArray<Run>): Conversation {
  let c = create(options);
  if (run !== undefined) c = setRun(c, run, children || []).conversation;
  return appendAll(c, events).conversation;
}

/**
 * What the state says about the run now: its status, the request it waits on, and its native
 * children. Call it when the conversation opens and whenever the store's copy of the run changes.
 */
export function setRun(conversation: Conversation, run: Run, children: ReadonlyArray<Run>): Appended {
  const w = begin(conversation);
  const before = w.c.attention;
  w.c.status = run.status;
  w.c.active = ['queued', 'starting', 'running'].includes(run.status);
  w.c.attention = run.attention && run.attention.kind === 'permission' ? run.attention.request_id : undefined;
  for (const c of children) {
    const info: ChildInfo = { id: c.id, title: c.title, status: c.status, parent: c.parent_run_id, evidence: c.relation_source };
    const was = mapGet(w.c.childInfo, c.id);
    if (was === undefined || JSON.stringify(was) !== JSON.stringify(info)) w.c.childInfo = mapSet(w.c.childInfo, c.id, info);
    w.c.family = mapSet(w.c.family, c.id, true);
    childHeader(w, c.id);
  }
  if (before !== w.c.attention) {
    if (before !== undefined) renderCard(w, before);
    if (w.c.attention !== undefined) renderCard(w, w.c.attention);
  }
  working(w);
  return finish(w, conversation);
}

function add(w: Work, ev: DaemonEvent): void {
  const p = record(ev.payload);
  const child = !!ev.run_id && ev.run_id !== w.c.rootId;
  const run = ev.run_id || w.c.rootId;
  const t = TEXT.conversation;
  switch (ev.kind) {
    case 'turn_started': {
      if (child) break;
      w.c.stopping = false;
      const turn = record(p['turn']);
      const given = p['turn'] ? { n: turn['n'] as number | string, prompt: turn['prompt'] } : { n: w.c.turns.length + 1, prompt: '' };
      newTurn(w, given, ev, false);
      w.c.active = true;
      working(w, t.working);
      break;
    }
    case 'interrupt_requested':
      if (!child) w.c.stopping = true;
      break;
    case 'remote_command': {
      // What was done from a phone, in the owner's words. For a message, the turn that follows is that message.
      if (!child && p['method'] === 'run.follow_up' && typeof p['request_id'] === 'string') w.c.arriving = p['request_id'];
      const who = String(ev.source || '').replace(/^phone:/, '').trim() || t.aPhone;
      const what = typeof p['method'] === 'string' && Object.hasOwn(t.fromPhone, p['method']) ? t.fromPhone[p['method']] : undefined;
      const place = placeFor(w, ev);
      put(w, place.end, [{ ...base(place, `note:${ev.seq}`, run, ev.seq), kind: 'note', text: what ? t.doneFrom(what, who) : t.from(who), status: null, icon: null, tooltip: null }]);
      break;
    }
    case 'output': {
      const role = String(p['role'] || 'assistant');
      if (role === 'system') break;
      const place = placeFor(w, ev);
      const body = String(p['text'] || '');
      if (role === 'reasoning' || role === 'plan') {
        const row: ThinkingRow = { ...base(place, `think:${ev.seq}`, run, ev.seq), kind: 'thinking', role, label: role === 'plan' ? t.plan : t.thinking, icon: role === 'plan' ? 'checklist' : 'lightbulb', text: body };
        put(w, place.end, [row]);
      } else {
        const row: MessageRow = { ...base(place, `msg:${ev.seq}`, run, ev.seq), kind: 'message', role, label: role === 'assistant' ? (child ? t.subAgent : t.agent) : role, text: body, markdown: role === 'assistant' };
        put(w, place.end, [row]);
        if (!child) working(w, t.working);
      }
      break;
    }
    case 'tool': {
      let card = toolCard(w, ev, p['id'], p['name']);
      const summary = String(p['summary'] || '');
      const st = /\[(completed|failed|declined|inProgress|in_progress|running|error)[^\]]*\]\s*$/.exec(summary);
      card = { ...card, name: p['name'] ? String(p['name']) : card.name, summary, status: st ? st[1] : card.status || 'running' };
      w.c.tools = mapSet(w.c.tools, card.key, card);
      label(w, card.key);
      if (!child) {
        const d = describe(card.name, card.input, card.summary);
        working(w, `${d.pending || d.verb} ${d.target || ''}`.trim() + '…');
      }
      break;
    }
    case 'tool_result': {
      let card = toolCard(w, ev, p['id'], undefined);
      const has = (k: string): boolean => p[k] !== undefined && p[k] !== null;
      card = {
        ...card, input: has('input') ? p['input'] : card.input, output: has('output') ? p['output'] : card.output, reported: has('status') ? p['status'] : card.reported,
        isError: has('is_error') ? p['is_error'] : card.isError, status: p['is_error'] ? 'failed' : String(p['status'] || 'completed'),
      };
      w.c.tools = mapSet(w.c.tools, card.key, card);
      label(w, card.key);
      break;
    }
    case 'file_activity': {
      const place = placeFor(w, ev);
      const group = lastGroup(w.c, place);
      const paths = Array.isArray(p['paths']) ? p['paths'] : [];
      const row: EditRow = {
        ...base(place, `edit:${ev.seq}`, run, ev.seq, group !== undefined ? group.key : place.owner), kind: 'edit', icon: 'diff', confidence: ev.confidence, group: group !== undefined ? group.key : null,
        files: paths.map(path => ({ name: basename(path), path: String(path), tooltip: `${String(path)}\n${t.openAtHunk}` })),
      };
      put(w, place.end, [row]);
      if (group !== undefined) w.c.groups = mapSet(w.c.groups, group.key, { ...(mapGet(w.c.groups, group.key) as GroupState), edits: group.edits + 1 });
      break;
    }
    case 'permission': {
      const place = placeFor(w, ev);
      const key = `perm:${ev.seq}`;
      const card: CardState = { key, answer: undefined, by: null, at: place.end };
      w.c.cards = mapSet(w.c.cards, String(p['request_id']), card);
      put(w, place.end, [cardRow(base(place, key, run, ev.seq), p['request_id'], p['tool'], p['input'], card, w.c)]);
      break;
    }
    case 'permission_answered': {
      const card = mapGet(w.c.cards, String(p['request_id']));
      if (card === undefined) break;
      w.c.cards = mapSet(w.c.cards, String(p['request_id']), { ...card, answer: p['allow'] ? 'allowed' : 'denied', by: typeof p['by'] === 'string' ? p['by'] : null });
      renderCard(w, p['request_id']);
      break;
    }
    case 'error': {
      // Quiet endings: a stop is not an error, and an error with nothing to say shows nothing.
      if (!child && (w.c.stopping || !String(p['message'] || '').trim())) break;
      if (!child) turnChange(w, ensureTurn(w, ev), { hadError: true });
      const place = placeFor(w, ev);
      const cls = typeof p['class'] === 'string' ? p['class'] : '';
      const row: ErrorRow = { ...base(place, `err:${ev.seq}`, run, ev.seq), kind: 'error', icon: 'error', class: cls || 'error', title: t.errorTitle[cls] || t.errorTitleOther, message: plain(p['message'] || '', 600), signIn: cls === 'auth' };
      put(w, place.end, [row]);
      break;
    }
    case 'child': {
      const c = record(p['child']);
      if (typeof c['id'] !== 'string') break;
      childBlock(w, c['id'], {
        id: c['id'], title: c['title'] as string | undefined, status: c['status'] as string | undefined, native_id: c['native_id'] as string | undefined, parent: c['parent_run_id'] as string | undefined,
        evidence: p['evidence'] || c['relation_source'], confidence: c['relation_confidence'] as string | undefined,
      }, ev);
      w.c.family = mapSet(w.c.family, c['id'], true);
      break;
    }
    case 'child_reparented': {
      const moved = String(p['child_run_id']), to = String(p['parent_run_id']);
      const block = mapGet(w.c.blocks, moved);
      if (block === undefined) break;
      w.c.childInfo = mapSet(w.c.childInfo, moved, { ...mapGet(w.c.childInfo, moved), parent: to });
      if (to === w.c.rootId) break;
      const parent = childBlock(w, to, undefined, ev);
      const from = locate(w.c.rows, block.key, block.at);
      const under = locate(w.c.rows, parent.key, parent.at);
      const inside = (outer: number, inner: number): boolean => inner >= outer && inner < endOfSubtree(w.c, outer);
      if (inside(under, from) || inside(from, under)) break;
      move(w, moved, { owner: parent.key, under });
      w.c.blocks = mapSet(w.c.blocks, moved, { ...(mapGet(w.c.blocks, moved) as BlockState), nested: 'reparented' });
      break;
    }
    case 'turn_done': {
      if (child) break;
      const ordinal = ensureTurn(w, ev);
      const turn = w.c.turns[ordinal] as TurnState;
      // A turn the user stopped reads "Stopped"; a failed one shows its reason once.
      const ok = !!p['ok'];
      const stopped = !ok && w.c.stopping;
      const word = ok ? t.done : stopped ? t.stopped : t.failed;
      // The harness's words in plain words (AC-245).
      const reason = !ok && !stopped && !turn.hadError && p['summary'] ? plain(String(p['summary']).split('\n')[0], 400).slice(0, 160) : '';
      const change: Writable<Partial<FooterRow>> = { state: ok ? 'ok' : stopped ? 'stopped' : 'fail', icon: ok ? 'check' : stopped ? 'circle-slash' : 'error', text: reason ? `${word}: ${reason}` : word };
      if (p['summary'] && !ok) change.tooltip = plain(p['summary'], 400);
      w.c.stopping = false;
      const end = eventTime(ev);
      if (turn.started && end && end > turn.started) change.duration = duration(end - turn.started);
      turnChange(w, ordinal, { ended: true });
      foot(w, ordinal, change);
      w.c.active = false;
      working(w);
      for (const key of w.c.open) label(w, key);
      break;
    }
    case 'retention':
      w.c.banner = t.trimmed;
      break;
    default:
      break;
  }
  if (ev.kind === 'status') status(w, ev, p, child);
  if (ev.kind === 'usage') usage(w, ev, p, child);
  if (!QUIET.has(ev.kind) && !KNOWN.has(ev.kind)) {
    const place = placeFor(w, ev);
    put(w, place.end, [{ ...base(place, `note:${ev.seq}`, run, ev.seq), kind: 'note', text: ev.kind.replace(/_/g, ' '), status: null, icon: null, tooltip: null }]);
  }
}

function status(w: Work, ev: DaemonEvent, p: Readonly<Record<string, unknown>>, child: boolean): void {
  const st = String(p['status']);
  if (child) {
    const run = ev.run_id as string;
    w.c.childInfo = mapSet(w.c.childInfo, run, { ...mapGet(w.c.childInfo, run), status: p['status'] as string });
    childHeader(w, run);
    return;
  }
  if (['interrupted', 'failed', 'disconnected', 'unknown', 'completed'].includes(st)) { w.c.active = false; working(w); }
  if (['running', 'starting'].includes(st)) { w.c.active = true; working(w); }
  if (st === 'waiting_for_user') { w.c.active = false; working(w); }
  if (!['interrupted', 'failed', 'disconnected', 'unknown'].includes(st)) return;
  const ordinal = w.c.turns.length - 1;
  const turn = w.c.turns[ordinal];
  // The turn's footer already says how it ended; one line is enough.
  if (turn !== undefined && turn.ended && ['interrupted', 'failed'].includes(st)) return;
  if (turn !== undefined && st === 'interrupted') {
    turnChange(w, ordinal, { ended: true });
    foot(w, ordinal, { state: 'stopped', icon: 'circle-slash', text: TEXT.conversation.stopped });
    w.c.stopping = false;
    return;
  }
  const place = placeFor(w, ev);
  put(w, place.end, [{
    ...base(place, `status:${ev.seq}`, ev.run_id || w.c.rootId, ev.seq), kind: 'note', text: statusText(st), status: st, icon: st === 'interrupted' ? 'circle-slash' : 'error',
    tooltip: p['reason'] ? plain(p['reason'], 400) : null,
  }]);
}

function usage(w: Work, ev: DaemonEvent, p: Readonly<Record<string, unknown>>, child: boolean): void {
  const u = record(p['usage'] || p['total'] || p['tokens'] || p);
  const pick = (...keys: string[]): number | undefined => keys.map(k => u[k]).find(v => typeof v === 'number') as number | undefined;
  const input = pick('input_tokens', 'inputTokens', 'input'), output = pick('output_tokens', 'outputTokens', 'output');
  if (child) {
    // A sub-agent's usage stands beside its title: what it reported, never counted as allowance.
    const counts = [input, output];
    if (counts.every(v => v === undefined) || counts.some(v => v !== undefined && (!Number.isSafeInteger(v) || v < 0))) return;
    const total = (input || 0) + (output || 0);
    if (!Number.isSafeInteger(total)) return;
    const t = TEXT.conversation;
    const label = input !== undefined && output !== undefined ? t.childTokens(compact(total)) : t.childTokensOf(compact(total), input !== undefined);
    const detail = [input !== undefined && t.childInput(input), output !== undefined && t.childOutput(output), t.notAllowance].filter(Boolean).join(' · ');
    const run = ev.run_id as string;
    w.c.childUsage = mapSet(w.c.childUsage, run, { label, detail });
    childBlock(w, run, undefined, ev);
    childHeader(w, run);
    return;
  }
  const ordinal = ensureTurn(w, ev);
  const cached = pick('cache_read_input_tokens', 'cached_input_tokens', 'cachedInputTokens');
  const cost = typeof p['total_cost_usd'] === 'number' ? p['total_cost_usd'] : typeof p['cost'] === 'number' ? p['cost'] : undefined;
  if (input === undefined && output === undefined && cost === undefined) return;
  const t = TEXT.conversation;
  const parts: string[] = [];
  if (input !== undefined || output !== undefined) parts.push(t.tokens(compact((input || 0) + (output || 0))));
  if (cost) parts.push(`$${cost < 0.01 ? cost.toFixed(4) : cost.toFixed(2)}`);
  const detail = [input !== undefined && t.tokensIn(input), output !== undefined && t.tokensOut(output), cached !== undefined && t.tokensCached(cached), cost !== undefined && `$${cost.toFixed(4)}`].filter(Boolean).join(' · ');
  foot(w, ordinal, { usage: parts.join(' · '), usageDetail: detail });
}

// ------------------------------------------------------------------ what a row opens to

export interface ToolDetail {
  /** What the call was given: "Command", "Content", "New text" or "Input", and the text. */
  readonly input: { readonly label: string; readonly text: string } | null;
  /** What it returned: "Result" or "Error", and the text. */
  readonly output: { readonly label: string; readonly text: string; readonly error: boolean } | null;
  /** Said when there is no output: "Waiting for the result…", "No output reported." */
  readonly note: string | null;
}

/** What a tool call's row shows when it is opened. At most 20,000 characters of each, as in VS Code. */
export function toolDetail(row: ToolRow): ToolDetail {
  const t = TEXT.conversation;
  const given = row.input !== undefined && row.input !== null ? row.input : row.summary;
  let input: ToolDetail['input'] = null;
  if (given) {
    const parsed = parseInput(given);
    const shown = parsed['command'] || parsed['content'] || parsed['new_string'] || (typeof given === 'string' ? given : JSON.stringify(given, null, 2));
    input = { label: parsed['command'] ? t.command : parsed['content'] ? t.content : parsed['new_string'] ? t.newText : t.input, text: String(shown).slice(0, 20000) };
  }
  if (row.output) return { input, output: { label: row.isError ? t.error : t.result, text: String(row.output).slice(0, 20000), error: row.isError }, note: null };
  return { input, output: null, note: row.reported && row.reported !== 'started' ? t.noOutput : t.waitingForResult };
}

/** The exact request the agent sent, as text. At most 4,000 characters, as in VS Code. */
export function requestText(row: PermissionRow): string {
  return String(JSON.stringify(row.input === undefined ? null : row.input, null, 2)).slice(0, 4000);
}

/** The answers a pending request offers. */
export function permissionActions(row: PermissionRow): ReadonlyArray<{ readonly label: string; readonly allow: boolean }> {
  return row.state === 'pending' ? [{ label: TEXT.conversation.allowOnce, allow: true }, { label: TEXT.conversation.deny, allow: false }] : [];
}

const parsed = new WeakMap<Row, ReadonlyArray<Block>>();

/** The row's text as a Markdown tree. Parsed once for each row. */
export function markdownOf(row: MessageRow | ThinkingRow, conversation: Conversation): ReadonlyArray<Block> {
  const known = parsed.get(row);
  if (known !== undefined) return known;
  const blocks = parse(row.text, { home: conversation.home });
  parsed.set(row, blocks);
  return blocks;
}

export function rowCount(conversation: Conversation): number {
  return conversation.rows.length;
}

export { describe };
export type { ToolDescription };
