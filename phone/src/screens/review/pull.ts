import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useSession, useSessionValue } from '@/session';

import { notConnected, sentence } from './errors';
import { opened, pullPlan, type Opened, type PullPlan } from './plans';

export interface PullRequest {
  /** `null` until the Mac sent the plan. */
  readonly plan: PullPlan | null;
  readonly loading: boolean;
  /** True while the Mac pushes the branch and opens the pull request. */
  readonly opening: boolean;
  /** What the Mac refused, in its words, as one line. */
  readonly error: string | null;
  /** Set once the pull request is open. */
  readonly opened: Opened | null;
  reload(): Promise<void>;
  /** Opens it with the title and the description given. An empty description is written by the Mac. */
  open(title: string, body: string): Promise<void>;
}

/**
 * A pull request for a run's branch (daemon/src/pr.rs): the Mac plans, commits, pushes with
 * the owner's own Git and GitHub sign-in, and opens it. Nothing is merged.
 * `onPlan` is told each plan as it arrives.
 */
export function usePullRequest(workspaceId: string | undefined, onPlan: (plan: PullPlan) => void): PullRequest {
  const session = useSession();
  const online = useSessionValue((s) => s.connection === 'online');
  const [plan, setPlan] = useState<PullPlan | null>(null);
  const [settled, setSettled] = useState(false);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<Opened | null>(null);
  const generation = useRef(0);
  const alive = useRef(true);
  const tell = useRef(onPlan);
  useEffect(() => {
    tell.current = onPlan;
  }, [onPlan]);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const reload = useCallback(async (): Promise<void> => {
    const mine = ++generation.current;
    const current = (): boolean => alive.current && mine === generation.current;
    try {
      const answered = await session.request('workspace.pr_plan', { workspace_id: workspaceId ?? '' });
      if (!current()) return;
      const next = pullPlan(answered);
      setPlan(next);
      tell.current(next);
    } catch (failure) {
      if (!current()) return;
      setError((before) => (notConnected(failure) ? before : sentence(failure)));
    } finally {
      if (current()) setSettled(true);
    }
  }, [session, workspaceId]);

  useEffect(() => {
    if (workspaceId && online) void reload();
  }, [workspaceId, online, reload]);

  const open = useCallback(
    async (title: string, body: string): Promise<void> => {
      if (!workspaceId) return;
      setOpening(true);
      setError(null);
      try {
        const answered = opened(await session.request('workspace.pr_open', { workspace_id: workspaceId, title: title.trim(), ...(body.trim() ? { body } : {}) }));
        if (alive.current) setResult(answered);
      } catch (failure) {
        if (alive.current) setError(sentence(failure));
      } finally {
        if (alive.current) setOpening(false);
      }
    },
    [session, workspaceId],
  );

  return useMemo(() => ({ plan, loading: online && !settled && Boolean(workspaceId), opening, error, opened: result, reload, open }), [plan, online, settled, workspaceId, opening, error, result, reload, open]);
}
