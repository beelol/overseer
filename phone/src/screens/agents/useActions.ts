import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { isActive, record, store, text } from '@/model';
import type { Session } from '@/session';
import { useSession, useSessionValue } from '@/session';

import { UNLOCK_FAILED, useUnlockBeforeChanges } from '../settings/safety';
import { WORDS } from './words';

const NOTHING: ReadonlySet<string> = new Set();

function withId(set: ReadonlySet<string>, ids: readonly string[]): ReadonlySet<string> {
  if (ids.every((id) => set.has(id))) return set;
  return new Set([...set, ...ids]);
}

function withoutId(set: ReadonlySet<string>, ids: readonly string[]): ReadonlySet<string> {
  if (!ids.some((id) => set.has(id))) return set;
  const next = new Set(set);
  for (const id of ids) next.delete(id);
  return next.size === 0 ? NOTHING : next;
}

type State = ReturnType<Session['getSnapshot']>['state'];

const why = (error: unknown): string => (error instanceof Error ? error.message : String(error));

export interface AgentActions {
  /** Tasks whose archiving was asked for and is not in the Mac's state yet: their rows are gone already. */
  readonly archiving: ReadonlySet<string>;
  /** Runs whose stop was asked for and that have not stopped yet. */
  readonly stopping: ReadonlySet<string>;
  /** What the Mac refused, in one sentence; `null` when nothing was. */
  readonly error: string | null;
  archive(taskId: string): void;
  stop(runId: string): void;
  /** Stops every active agent. */
  stopAll(): void;
  /** The top-level agents that are going now: what Stop all stops. */
  activeAgents(): readonly string[];
}

/**
 * What the list changes on the Mac. Each change shows at once and is sent through the session,
 * which queues it and sends it once; what the Mac refuses is put back and said.
 */
export function useAgentActions(): AgentActions {
  const session = useSession();
  const outbox = useSessionValue((s) => s.outbox);
  const [archived, setArchived] = useState(NOTHING);
  const [stopped, setStopped] = useState(NOTHING);
  const [error, setError] = useState<string | null>(null);

  const watching = useRef(new Set<() => void>());
  useEffect(() => {
    const watched = watching.current;
    return () => {
      for (const off of watched) off();
      watched.clear();
    };
  }, []);

  /** Once the Mac's state shows a change, the state is what holds: the phone's note of it goes. */
  const until = useCallback(
    (shows: (state: State) => boolean, forget: () => void): (() => void) => {
      const off = session.subscribe(() => {
        if (!shows(session.getSnapshot().state)) return;
        stop_();
        forget();
      });
      const stop_ = (): void => {
        off();
        watching.current.delete(stop_);
      };
      watching.current.add(stop_);
      return stop_;
    },
    [session],
  );

  // What was asked for before this screen opened and still waits to be sent.
  const waiting = useMemo(() => {
    const tasks: string[] = [];
    const runs: string[] = [];
    let all = false;
    for (const entry of outbox) {
      if (entry.state !== 'queued' && entry.state !== 'sending') continue;
      const params = record(entry.params);
      if (entry.method === 'task.archive' && params['archived'] !== false && typeof params['task_id'] === 'string') tasks.push(params['task_id']);
      else if (entry.method === 'run.interrupt' && typeof params['run_id'] === 'string') runs.push(params['run_id']);
      else if (entry.method === 'runs.stop_all') all = true;
    }
    return { tasks, runs, all };
  }, [outbox]);

  const activeAgents = useCallback(
    (): readonly string[] =>
      store
        .rows(session.getSnapshot().state.runs)
        .filter((run) => !run.parent_run_id && isActive(run.status))
        .map((run) => run.id),
    [session],
  );

  const archiving = useMemo(() => withId(archived, waiting.tasks), [archived, waiting]);
  const stopping = useMemo(() => withId(stopped, waiting.all ? [...waiting.runs, ...activeAgents()] : waiting.runs), [stopped, waiting, activeAgents]);

  const archive = useCallback(
    (taskId: string) => {
      setError(null);
      setArchived((now) => withId(now, [taskId]));
      const forget = (): void => setArchived((now) => withoutId(now, [taskId]));
      const off = until((state) => Boolean(store.task(state, taskId)?.archived_ms), forget);
      session.request('task.archive', { task_id: taskId, archived: true }).catch((refused: unknown) => {
        off();
        forget();
        setError(WORDS.notDone(text.TEXT.agents.archive, why(refused)));
      });
    },
    [session, until],
  );

  const stop = useCallback(
    (runId: string) => {
      setError(null);
      setStopped((now) => withId(now, [runId]));
      const forget = (): void => setStopped((now) => withoutId(now, [runId]));
      const off = until((state) => !isActive(store.run(state, runId)?.status), forget);
      session.request('run.interrupt', { run_id: runId }).catch((refused: unknown) => {
        off();
        forget();
        setError(WORDS.notDone(text.TEXT.agents.stop, why(refused)));
      });
    },
    [session, until],
  );

  const unlockFirst = useUnlockBeforeChanges();
  const stopAll = useCallback(async () => {
    const unlocked = await unlockFirst(WORDS.stopAll);
    if (!unlocked.ok) {
      if (unlocked.cause !== 'cancelled') setError(UNLOCK_FAILED);
      return;
    }
    const ids = activeAgents();
    setError(null);
    setStopped((now) => withId(now, ids));
    const offs = ids.map((id) => until((state) => !isActive(store.run(state, id)?.status), () => setStopped((now) => withoutId(now, [id]))));
    session.request('runs.stop_all', {}).catch((refused: unknown) => {
      for (const off of offs) off();
      setStopped((now) => withoutId(now, ids));
      setError(WORDS.notDone(WORDS.stopAll, why(refused)));
    });
  }, [session, activeAgents, until, unlockFirst]);

  return useMemo(() => ({ archiving, stopping, error, archive, stop, stopAll, activeAgents }), [archiving, stopping, error, archive, stop, stopAll, activeAgents]);
}
