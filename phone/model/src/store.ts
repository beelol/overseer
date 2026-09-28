// The phone's copy of the daemon's state, kept current from events alone.
//
//   load(state)          the daemon's `state`, as the phone keeps it
//   apply(state, event)  the state after one event
//   snapshot(state)      the same records in the daemon's own shape and order
//
// After any sequence of events the snapshot equals what the daemon's `state` returns at that
// moment (test/store.test.ts checks it against recordings of a real daemon). Both functions are
// pure. An event changes one record and the small tables on the way to it; nothing large is
// copied, and most events (output, tools, usage) change nothing but the cursor.

import { mapDelete, mapGet, mapSet, pmap, vecArray, vecFrom, vecGet, vecSplice } from './persistent.ts';
import type { PMap, PVec } from './persistent.ts';
import { isActive, isAttention, isMark, isRun, isTurn, number, record, text } from './types.ts';
import type { DaemonEvent, Mark, Profile, Run, RunStatus, State, Task, Turn, Workspace } from './types.ts';

/** Records by id, and their ids in the daemon's order. */
export interface Table<T> {
  readonly byId: PMap<T>;
  readonly order: PVec<string>;
}

export interface PhoneState {
  /** The last event this state includes. */
  readonly cursor: number;
  readonly daemon: State['daemon'] | null;
  readonly tasks: Table<Task>;
  readonly runs: Table<Run>;
  readonly workspaces: Table<Workspace>;
  readonly profiles: Table<Profile>;
  /** The turns of each top-level run, by run id. */
  readonly turns: PMap<ReadonlyArray<Turn>>;
  /** The ids of each run's native children, in the daemon's order, by the parent's id. */
  readonly kids: PMap<ReadonlyArray<string>>;
  /** Hunks marked reviewed, by run id, for the runs whose marks were loaded. */
  readonly marks: PMap<ReadonlyArray<Mark>>;
  /** Runs asked to stop whose process has not been replaced since: their turn ends as interrupted. */
  readonly stopping: PMap<true>;
}

type Row = { readonly id: string; readonly created_ms: number };

const byCreated = (a: Row, b: Row): number => a.created_ms - b.created_ms;
const byCreatedThenId = (a: Row, b: Row): number => a.created_ms - b.created_ms || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

function table<T extends Row>(rows: ReadonlyArray<T>): Table<T> {
  let byId = pmap<T>();
  for (const row of rows) byId = mapSet(byId, row.id, row);
  return { byId, order: vecFrom(rows.map(r => r.id)) };
}

/** Puts a record where the daemon's `ORDER BY` puts it; one that is already there is replaced. */
function insert<T extends Row>(t: Table<T>, row: T, compare: (a: Row, b: Row) => number): Table<T> {
  if (mapGet(t.byId, row.id) !== undefined) return { byId: mapSet(t.byId, row.id, row), order: t.order };
  let at = t.order.length;
  // New records are nearly always the newest: look from the end.
  while (at > 0) {
    const before = mapGet(t.byId, vecGet(t.order, at - 1) as string);
    if (before === undefined || compare(before, row) <= 0) break;
    at--;
  }
  return { byId: mapSet(t.byId, row.id, row), order: vecSplice(t.order, at, 0, [row.id]) };
}

function replace<T extends Row>(t: Table<T>, row: T): Table<T> {
  return { byId: mapSet(t.byId, row.id, row), order: t.order };
}

function remove<T extends Row>(t: Table<T>, id: string): Table<T> {
  if (mapGet(t.byId, id) === undefined) return t;
  const at = vecArray(t.order).indexOf(id);
  return { byId: mapDelete(t.byId, id), order: at < 0 ? t.order : vecSplice(t.order, at, 1, []) };
}

const lists = new WeakMap<object, ReadonlyArray<unknown>>();

/** The records of a table in the daemon's order. Built once for each version of the table. */
export function rows<T>(t: Table<T>): ReadonlyArray<T> {
  const known = lists.get(t);
  if (known !== undefined) return known as ReadonlyArray<T>;
  const out: T[] = [];
  for (const id of vecArray(t.order)) {
    const row = mapGet(t.byId, id);
    if (row !== undefined) out.push(row);
  }
  lists.set(t, out);
  return out;
}

export const EMPTY: PhoneState = {
  cursor: 0, daemon: null, tasks: table<Task>([]), runs: table<Run>([]), workspaces: table<Workspace>([]), profiles: table<Profile>([]),
  turns: pmap(), kids: pmap(), marks: pmap(), stopping: pmap(),
};

/** The daemon's `state`, as the phone keeps it. */
export function load(state: State): PhoneState {
  let turns = pmap<ReadonlyArray<Turn>>();
  for (const [runId, list] of Object.entries(state.turns || {})) turns = mapSet(turns, runId, list);
  let kids = pmap<ReadonlyArray<string>>();
  for (const run of state.runs) if (run.parent_run_id) kids = mapSet(kids, run.parent_run_id, [...(mapGet(kids, run.parent_run_id) || []), run.id]);
  return {
    cursor: state.cursor, daemon: state.daemon ?? null, tasks: table(state.tasks), runs: table(state.runs), workspaces: table(state.workspaces), profiles: table(state.profiles),
    turns, kids, marks: pmap(), stopping: pmap(),
  };
}

/** The marks of a run as `review.marks` returned them. */
export function loadMarks(state: PhoneState, runId: string, marks: ReadonlyArray<Mark>): PhoneState {
  return { ...state, marks: mapSet(state.marks, runId, marks.filter(isMark)) };
}

/** The state in the daemon's own shape: what `state` returns, without the cursor's daemon block when it was never loaded. */
export function snapshot(state: PhoneState): Pick<State, 'cursor' | 'tasks' | 'runs' | 'workspaces' | 'profiles' | 'turns'> & { daemon: State['daemon'] | null } {
  const turns: Record<string, Turn[]> = {};
  for (const run of rows(state.runs)) if (!run.parent_run_id) turns[run.id] = [...(mapGet(state.turns, run.id) || [])];
  return { cursor: state.cursor, daemon: state.daemon, tasks: [...rows(state.tasks)], runs: [...rows(state.runs)], workspaces: [...rows(state.workspaces)], profiles: [...rows(state.profiles)], turns };
}

export const run = (state: PhoneState, id: string | null | undefined): Run | undefined => (id ? mapGet(state.runs.byId, id) : undefined);
export const task = (state: PhoneState, id: string | null | undefined): Task | undefined => (id ? mapGet(state.tasks.byId, id) : undefined);
export const workspace = (state: PhoneState, id: string | null | undefined): Workspace | undefined => (id ? mapGet(state.workspaces.byId, id) : undefined);
export const profile = (state: PhoneState, id: string | null | undefined): Profile | undefined => (id ? mapGet(state.profiles.byId, id) : undefined);
export const turnsOf = (state: PhoneState, runId: string): ReadonlyArray<Turn> => mapGet(state.turns, runId) || NONE;
export const marksOf = (state: PhoneState, runId: string): ReadonlyArray<Mark> => mapGet(state.marks, runId) || NONE;

const NONE: ReadonlyArray<never> = Object.freeze([]);

/** A run's native children, in the daemon's order. */
export function childrenOf(state: PhoneState, runId: string): ReadonlyArray<Run> {
  const ids = mapGet(state.kids, runId);
  if (ids === undefined) return NONE;
  const out: Run[] = [];
  for (const id of ids) {
    const child = mapGet(state.runs.byId, id);
    if (child !== undefined) out.push(child);
  }
  return out;
}

/** Children, their children and so on, nearest first (the order of the extension's `descendants`). */
export function descendantsOf(state: PhoneState, runId: string): ReadonlyArray<Run> {
  const out: Run[] = [];
  const queue = [runId];
  const seen = new Set<string>();
  for (let id = queue.shift(); id !== undefined; id = queue.shift()) {
    if (seen.has(id)) continue;
    seen.add(id);
    for (const child of childrenOf(state, id)) {
      out.push(child);
      queue.push(child.id);
    }
  }
  return out;
}

/** The top-level run a run belongs to. */
export function rootOf(state: PhoneState, runId: string): Run | undefined {
  let current = run(state, runId);
  const seen = new Set<string>();
  while (current?.parent_run_id && !seen.has(current.id)) {
    seen.add(current.id);
    current = run(state, current.parent_run_id);
  }
  return current;
}

function withRun(state: PhoneState, next: Run): PhoneState {
  return { ...state, runs: replace(state.runs, next) };
}

function addKid(kids: PMap<ReadonlyArray<string>>, state: PhoneState, parent: string, child: Run): PMap<ReadonlyArray<string>> {
  const now = mapGet(kids, parent) || NONE;
  if (now.includes(child.id)) return kids;
  // The daemon lists children by creation time, then id.
  const next = [...now, child.id].sort((a, b) => {
    const ra = a === child.id ? child : mapGet(state.runs.byId, a);
    const rb = b === child.id ? child : mapGet(state.runs.byId, b);
    return ra !== undefined && rb !== undefined ? byCreatedThenId(ra, rb) : 0;
  });
  return mapSet(kids, parent, next);
}

function dropKid(kids: PMap<ReadonlyArray<string>>, parent: string, child: string): PMap<ReadonlyArray<string>> {
  const now = mapGet(kids, parent);
  if (now === undefined || !now.includes(child)) return kids;
  const next = now.filter(id => id !== child);
  return next.length ? mapSet(kids, parent, next) : mapDelete(kids, parent);
}

/** Turns that have not ended end now, the way the daemon's `finish_open_turns` ends them. */
function finishTurns(state: PhoneState, runId: string, status: string, ms: number): PMap<ReadonlyArray<Turn>> {
  const turns = mapGet(state.turns, runId);
  if (turns === undefined || turns.every(t => t.ended_ms !== null && t.ended_ms !== undefined)) return state.turns;
  return mapSet(state.turns, runId, turns.map(t => (t.ended_ms === null || t.ended_ms === undefined ? { ...t, status, ended_ms: ms } : t)));
}

function setOwner(state: PhoneState, workspaceId: string, owner: string | null, onlyIf?: string): Table<Workspace> {
  const ws = mapGet(state.workspaces.byId, workspaceId);
  if (ws === undefined || (onlyIf !== undefined && ws.owner_run_id !== onlyIf) || (ws.owner_run_id ?? null) === owner) return state.workspaces;
  return replace(state.workspaces, { ...ws, owner_run_id: owner });
}

/**
 * The state after one event. An event at or before the cursor is already included and changes
 * nothing, so a replay that overlaps is harmless. An event of a kind this does not know, or one
 * that names a record it does not have, moves the cursor and nothing else.
 */
export function apply(state: PhoneState, event: DaemonEvent): PhoneState {
  if (typeof event.seq !== 'number' || event.seq <= state.cursor) return state;
  const next = change(state, event);
  return next === state ? { ...state, cursor: event.seq } : { ...next, cursor: event.seq };
}

/** Several events, in order. */
export function applyAll(state: PhoneState, events: Iterable<DaemonEvent>): PhoneState {
  let next = state;
  for (const event of events) next = apply(next, event);
  return next;
}

function change(state: PhoneState, event: DaemonEvent): PhoneState {
  const p = record(event.payload);
  const runId = event.run_id ?? undefined;
  switch (event.kind) {
    case 'task_created': {
      const t = record(p['task']), w = record(p['workspace']);
      if (!isRun(p['run']) || typeof t['id'] !== 'string' || typeof w['id'] !== 'string') return state;
      const made = p['run'];
      // The daemon makes the run the owner of its workspace right after it stores both.
      const ws = { ...(w as unknown as Workspace), owner_run_id: made.id };
      return {
        ...state,
        tasks: insert(state.tasks, t as unknown as Task, byCreated),
        workspaces: insert(state.workspaces, ws, byCreated),
        runs: insert(state.runs, made, byCreatedThenId),
        turns: made.parent_run_id ? state.turns : mapSet(state.turns, made.id, mapGet(state.turns, made.id) || NONE),
      };
    }
    case 'turn_started': {
      if (!isTurn(p['turn'])) return state;
      const turn = p['turn'];
      const now = mapGet(state.turns, turn.run_id) || NONE;
      if (now.some(t => t.id === turn.id)) return state;
      return { ...state, turns: mapSet(state.turns, turn.run_id, [...now, turn].sort((a, b) => a.n - b.n)) };
    }
    case 'status': {
      const r = run(state, runId);
      const status = text(p['status']);
      if (r === undefined || status === undefined) return state;
      const reason = text(p['reason']);
      const generation = number(p['generation']);
      const ended = isActive(status) ? null : event.ts;
      let changed: Run = { ...r, status: status as RunStatus, ended_ms: ended, exit_reason: reason ?? r.exit_reason ?? null };
      let next: PhoneState = state;
      if (generation !== undefined) {
        // A new process: what the last one left is cleared, and this run owns its workspace.
        changed = { ...changed, process_generation: generation, exit_reason: null, ended_ms: null, attention: null };
        next = { ...next, workspaces: setOwner(next, r.workspace_id, r.id), stopping: mapDelete(next.stopping, r.id) };
      }
      if (!r.parent_run_id && ended !== null) {
        // The process of a top-level run ended: nothing is asked any more, its open turn ends the
        // same way, and the workspace has no owner.
        changed = { ...changed, attention: null };
        next = { ...next, turns: finishTurns(next, r.id, status, event.ts), workspaces: setOwner(next, r.workspace_id, null, r.id) };
      }
      return withRun(next, changed);
    }
    case 'reattached': {
      const r = run(state, runId);
      if (r === undefined || r.status !== 'disconnected') return state;
      return withRun(state, { ...r, status: 'running', ended_ms: null });
    }
    case 'session': {
      const r = run(state, runId);
      const native = text(p['native_id']);
      if (r === undefined || native === undefined || (r.native_id !== null && r.native_id !== undefined)) return state;
      return withRun(state, { ...r, native_id: native });
    }
    case 'permission': {
      const r = run(state, runId);
      if (r === undefined || !isAttention(event.payload)) return state;
      return withRun(state, { ...r, attention: event.payload });
    }
    case 'permission_answered': {
      const r = run(state, runId);
      if (r === undefined || !r.attention) return state;
      if (String(r.attention.request_id) !== String(p['request_id'])) return state;
      return withRun(state, { ...r, attention: null });
    }
    case 'interrupt_requested':
      return runId && run(state, runId) !== undefined ? { ...state, stopping: mapSet(state.stopping, runId, true) } : state;
    case 'turn_done': {
      if (!runId) return state;
      const status = p['ok'] === true ? 'completed' : mapGet(state.stopping, runId) ? 'interrupted' : 'failed';
      const turns = finishTurns(state, runId, status, event.ts);
      return turns === state.turns ? state : { ...state, turns };
    }
    case 'child': {
      if (!isRun(p['child'])) return state;
      const child = p['child'];
      if (run(state, child.id) !== undefined) return state;
      return { ...state, runs: insert(state.runs, child, byCreatedThenId), kids: child.parent_run_id ? addKid(state.kids, state, child.parent_run_id, child) : state.kids };
    }
    case 'child_reparented': {
      const child = run(state, text(p['child_run_id']));
      const parent = text(p['parent_run_id']);
      if (child === undefined || parent === undefined) return state;
      const moved: Run = { ...child, parent_run_id: parent, relation_confidence: 'exact (structured harness event; parent reported later)' };
      const kids = addKid(child.parent_run_id ? dropKid(state.kids, child.parent_run_id, child.id) : state.kids, state, parent, moved);
      return { ...withRun(state, moved), kids };
    }
    case 'task_archived': {
      const t = task(state, event.task_id);
      if (t === undefined) return state;
      return { ...state, tasks: replace(state.tasks, { ...t, archived_ms: p['archived'] === false ? null : event.ts }) };
    }
    case 'workspace_removed': {
      const ws = workspace(state, text(p['workspace_id']));
      if (ws === undefined) return state;
      return { ...state, workspaces: replace(state.workspaces, { ...ws, removed_ms: event.ts }) };
    }
    case 'profile': {
      const added = record(p['profile']);
      if (typeof added['id'] === 'string' && typeof added['created_ms'] === 'number') return { ...state, profiles: insert(state.profiles, added as unknown as Profile, byCreatedThenId) };
      const id = text(p['profile_id']);
      if (id !== undefined && p['action'] === 'removed') return { ...state, profiles: remove(state.profiles, id) };
      return state;
    }
    case 'review_mark': {
      const key = text(p['key']), path = text(p['path']);
      if (!runId || key === undefined) return state;
      const now = mapGet(state.marks, runId);
      // Marks are kept for the runs whose marks were loaded; for another run there is nothing to keep current.
      if (now === undefined) return state;
      const others = now.filter(m => m.key !== key);
      if (p['reviewed'] === false) return others.length === now.length ? state : { ...state, marks: mapSet(state.marks, runId, others) };
      return { ...state, marks: mapSet(state.marks, runId, [...others, { key, path: path ?? '', at_ms: event.ts, by: event.source === 'user' ? 'the Mac' : event.source }]) };
    }
    case 'review_reject': {
      const key = text(p['key']);
      const now = runId ? mapGet(state.marks, runId) : undefined;
      if (!runId || key === undefined || now === undefined || !now.some(m => m.key === key)) return state;
      return { ...state, marks: mapSet(state.marks, runId, now.filter(m => m.key !== key)) };
    }
    default:
      return state;
  }
}

/** The app saw, in a run's history, that its stop was asked for before this state was loaded. */
export function markStopping(state: PhoneState, runId: string): PhoneState {
  return mapGet(state.stopping, runId) ? state : { ...state, stopping: mapSet(state.stopping, runId, true) };
}
