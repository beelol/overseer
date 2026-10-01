import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { agents, store, text } from '@/model';
import { useSessionValue, type SessionSnapshot } from '@/session';
import { useMinute } from '@/ui';

import { toggled, useAgentsStore, withSeen } from './store';
import { useStored } from './stored';
import { useSearch } from './useSearch';

type Row = agents.AgentRow;

const NO_SEEN: Record<string, number> = {};
const NO_IDS: string[] = [];

let lastListed: store.PhoneState | undefined;

/**
 * The state as far as the list depends on it: the same object until a task, a run, a worktree,
 * an account or a child changes. An agent that writes a hundred lines a second changes none of
 * them, so the list is not drawn again for what it does not show.
 */
export function listed(snapshot: SessionSnapshot): store.PhoneState {
  const now = snapshot.state;
  const was = lastListed;
  if (was !== undefined && was.tasks === now.tasks && was.runs === now.runs && was.workspaces === now.workspaces && was.profiles === now.profiles && was.kids === now.kids) return was;
  lastListed = now;
  return now;
}

const isHeading = (row: Row): boolean => row.kind === 'section' || row.kind === 'repo';

/** The rows without the agents of `tasks`, and without a heading that has nothing left under it. */
export function withoutTasks(rows: readonly Row[], tasks: ReadonlySet<string>): readonly Row[] {
  if (tasks.size === 0) return rows;
  const kept = rows.filter((row) => row.taskId === null || !tasks.has(row.taskId));
  if (kept.length === rows.length) return rows;
  return kept.filter((row, at) => {
    if (!isHeading(row) || !row.expanded) return true;
    const next = kept[at + 1];
    return next !== undefined && next.depth > row.depth;
  });
}

export interface AgentsList {
  readonly rows: readonly Row[];
  /** What to say when there are no rows; `null` when there are, or nothing is known yet. */
  readonly empty: string | null;
  readonly counts: { readonly active: number; readonly needs: number };
  readonly filter: agents.AgentFilter;
  setFilter(filter: agents.AgentFilter): void;
  readonly query: string;
  setQuery(query: string): void;
  /** How many agents the search shows; `null` while nothing is searched. */
  readonly matches: number | null;
  readonly pinned: readonly string[];
  /**
   * The owner opened an agent: it is remembered, and an agent at its end is reviewed (AC-254). A
   * sub-agent's opening reviews the agent it belongs to, as in VS Code.
   */
  opened(runId: string): void;
  pin(runId: string, on: boolean): void;
  /** Folds or unfolds a heading. */
  fold(rowId: string, folded: boolean): void;
}

/**
 * The agents list: the model's rows for the daemon's state as the phone holds it, the filter,
 * the search and what the owner keeps here (opened, pinned, folded). Nothing is computed that
 * the model gives, and the rows are the same array until something they depend on changes.
 */
export function useAgentsList(hidden: ReadonlySet<string>): AgentsList {
  const state = useSessionValue(listed);
  const known = useSessionValue((s) => s.stateAt !== null);
  const now = useMinute();
  const kept = useAgentsStore();
  const [seen, setSeen] = useStored(kept, 'seen', NO_SEEN);
  const [pinned, setPinned] = useStored(kept, 'pinned', NO_IDS);
  const [folded, setFolded] = useStored(kept, 'collapsed', NO_IDS);
  const [filter, setFilter] = useState<agents.AgentFilter>('all');
  const [query, setQuery] = useState('');
  const found = useSearch(query);

  const collapsed = useMemo(() => new Set(folded), [folded]);
  const options = useMemo<agents.AgentsOptions>(() => ({ now, filter, query, matches: found, collapsed, seen, pinned }), [now, filter, query, found, collapsed, seen, pinned]);
  const all = useMemo(() => agents.agentRows(state, options), [state, options]);
  const rows = useMemo(() => withoutTasks(all, hidden), [all, hidden]);
  const counts = useMemo(() => agents.counts(state, { now, seen }), [state, now, seen]);
  const empty = useMemo(() => (rows.length > 0 || !known ? null : (agents.emptyText(state, options) ?? text.TEXT.agents.empty)), [rows, known, state, options]);
  const matches = useMemo(() => (query.trim() ? rows.filter((row) => row.kind === 'agent').length : null), [rows, query]);

  // The state as it is when the owner taps, without making a new callback for every change of it.
  const latest = useRef(state);
  useEffect(() => {
    latest.current = state;
  }, [state]);
  const opened = useCallback((runId: string) => setSeen((was) => withSeen(was, store.rootOf(latest.current, runId)?.id ?? runId, Date.now())), [setSeen]);
  const pin = useCallback((runId: string, on: boolean) => setPinned((was) => toggled(was, runId, on)), [setPinned]);
  const fold = useCallback((rowId: string, on: boolean) => setFolded((was) => toggled(was, rowId, on)), [setFolded]);

  return { rows, empty, counts, filter, setFilter, query, setQuery, matches, pinned, opened, pin, fold };
}
