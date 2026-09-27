import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type { Change } from '@/protocol';
import { useSession, useSessionValue, type Session } from '@/session';

import { notConnected, sentence } from './errors';
import { mergePlan, mergeStep, merged, type Merged, type MergePlan, type MergeReady } from './plans';
import { WORDS } from './words';

export type MergeAction = 'prepare' | 'resume' | 'complete' | 'abort';

/** What a step came to, in one or two calm sentences. */
export interface Outcome {
  readonly tone: 'muted' | 'red' | 'green';
  readonly text: string;
}

/** What will land on the target: the comparison that shows it, and the files. */
export interface Landing {
  readonly base: string;
  readonly changes: readonly Change[];
}

export interface MergeBack {
  /** `null` until the Mac sent the plan. */
  readonly plan: MergePlan | null;
  readonly loading: boolean;
  /** The step on its way to the Mac, or `null`. */
  readonly busy: MergeAction | null;
  readonly outcome: Outcome | null;
  readonly landing: Landing | null;
  /** Set once the branch is merged. */
  readonly done: Merged | null;
  reload(): Promise<void>;
  run(action: MergeAction): Promise<void>;
}

/**
 * What lands is what the branch holds that the target does not: the comparison VS Code shows
 * before it asks. Known once the merge is ready.
 */
async function landingOf(session: Session, runId: string, workspaceId: string, plan: MergePlan): Promise<Landing | null> {
  if (!plan.ok || plan.state !== 'ready') return null;
  const offered = await session.request('comparison.options', { run_id: plan.runId ?? runId, branch: plan.target });
  const base = offered.options.find((o) => o.mode === 'branch_merge_base' && o.branch === plan.target && o.available)?.base;
  if (!base) return null;
  const diff = await session.request('workspace.diff', { workspace_id: workspaceId, base, status: false });
  return { base, changes: diff.changes };
}

/**
 * Merging back, step by step as the Mac has it (daemon/src/merge.rs): the plan, prepare in the
 * worktree, conflicts resolved there, then the merge into the target. Never by itself.
 */
export function useMergeBack(runId: string, workspaceId: string | undefined): MergeBack {
  const session = useSession();
  const online = useSessionValue((s) => s.connection === 'online');
  const [plan, setPlan] = useState<MergePlan | null>(null);
  const [settled, setSettled] = useState(false);
  const [busy, setBusy] = useState<MergeAction | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [landing, setLanding] = useState<Landing | null>(null);
  const [done, setDone] = useState<Merged | null>(null);
  const generation = useRef(0);
  const alive = useRef(true);
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
      const answered = await session.request('workspace.merge_plan', { workspace_id: workspaceId ?? '' });
      if (!current()) return;
      const next = mergePlan(answered);
      setPlan(next);
      const lands = await landingOf(session, runId, workspaceId ?? '', next);
      if (!current()) return;
      setLanding(lands);
    } catch (failure) {
      if (!current()) return;
      setOutcome((before) => (notConnected(failure) ? before : { tone: 'red', text: sentence(failure) }));
    } finally {
      if (current()) setSettled(true);
    }
  }, [session, runId, workspaceId]);

  useEffect(() => {
    if (workspaceId && online) void reload();
  }, [workspaceId, online, reload]);

  const run = useCallback(
    async (action: MergeAction): Promise<void> => {
      if (!workspaceId || !plan?.ok) return;
      const ready: MergeReady = plan;
      setBusy(action);
      setOutcome(null);
      try {
        const said = await step(action, ready);
        if (!alive.current) return;
        setOutcome(said);
      } catch (failure) {
        if (alive.current) setOutcome({ tone: 'red', text: sentence(failure) });
      } finally {
        if (alive.current) setBusy(null);
      }
      await reload();

      async function step(which: MergeAction, from: MergeReady): Promise<Outcome | null> {
        const params = { workspace_id: workspaceId ?? '' };
        switch (which) {
          case 'prepare': {
            const answered = mergeStep(await session.request('workspace.merge_prepare', { ...params, handoff: true }));
            if (answered.state !== 'conflicts') return null;
            return { tone: 'muted', text: answered.sent ? WORDS.merge.sentToAgent : WORDS.merge.resolveYourself(answered.why) };
          }
          case 'resume': {
            const answered = mergeStep(await session.request('workspace.merge_resolved', params));
            return answered.state === 'ready' ? null : { tone: 'red', text: WORDS.merge.marksRemain(answered.remaining.join(', ')) };
          }
          case 'complete': {
            const answered = merged(await session.request('workspace.merge_complete', params), from);
            if (alive.current) setDone(answered);
            return null;
          }
          case 'abort':
            await session.request('workspace.merge_abort', params);
            return { tone: 'muted', text: WORDS.merge.aborted };
        }
      }
    },
    [session, workspaceId, plan, reload],
  );

  return useMemo(() => ({ plan, loading: online && !settled && Boolean(workspaceId), busy, outcome, landing, done, reload, run }), [plan, online, settled, workspaceId, busy, outcome, landing, done, reload, run]);
}
