import { useEffect, useRef, useState } from 'react';

import { store, type Run } from '@/model';
import { useSession, useSessionValue, type Session } from '@/session';

/** As the side bar and the model do: only the 40 most recently finished agents are asked about. */
const RECENT = 40;
const WEEK = 7 * 86_400_000;

type Counts = Readonly<Record<string, number>>;
const NONE: Counts = Object.freeze({});

/** What was asked already, for the life of the connection's session: a finished run is asked once. */
const asked = new WeakMap<Session, Map<string, number>>();

const when = (run: Run): number => run.ended_ms || run.created_ms;
const keyOf = (run: Run): string => `${run.id}:${when(run)}`;

function known(session: Session): Map<string, number> {
  let map = asked.get(session);
  if (!map) {
    map = new Map();
    asked.set(session, map);
  }
  return map;
}

function countsOf(map: Map<string, number>): Counts {
  const out: Record<string, number> = {};
  for (const [key, files] of map) if (files > 0) out[key.slice(0, key.lastIndexOf(':'))] = files;
  return out;
}

/**
 * Changed files of the agents that finished lately, by run id: what makes a finished agent one
 * that needs the owner ("Review"). Asked of the Mac one at a time, once for each finished run,
 * and only while connected. The same object until a count arrives.
 */
export function useChanged(seen: Readonly<Record<string, number>>): Counts {
  const session = useSession();
  const runs = useSessionValue((s) => s.state.runs);
  const online = useSessionValue((s) => s.connection === 'online');
  const [changed, setChanged] = useState<Counts>(() => {
    const counts = countsOf(known(session));
    return Object.keys(counts).length > 0 ? counts : NONE;
  });
  const waiting = useRef<Run[]>([]);
  const working = useRef(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!online) return;
    const map = known(session);
    const now = Date.now();
    const finished = store
      .rows(runs)
      .filter((run) => !run.parent_run_id && run.status === 'completed')
      .sort((a, b) => when(b) - when(a))
      .slice(0, RECENT);
    for (const run of finished) {
      if (map.has(keyOf(run)) || now - when(run) >= WEEK) continue;
      if ((seen[run.id] ?? 0) >= when(run)) continue;
      if (store.task(session.getSnapshot().state, run.task_id)?.archived_ms) continue;
      map.set(keyOf(run), 0);
      waiting.current.push(run);
    }
    if (working.current || waiting.current.length === 0) return;
    working.current = true;
    void (async () => {
      for (let run = waiting.current.shift(); run !== undefined; run = waiting.current.shift()) {
        try {
          const answer = await session.request('workspace.changes', { workspace_id: run.workspace_id });
          if (answer.files > 0) {
            map.set(keyOf(run), answer.files);
            if (mounted.current) setChanged(countsOf(map));
          }
        } catch {
          // A worktree that was removed has no changes to review.
        }
      }
      working.current = false;
    })();
  }, [session, runs, online, seen]);

  return changed;
}
