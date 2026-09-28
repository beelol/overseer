import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { review, text, type Hunk } from '@/model';
import type { Change } from '@/protocol';
import { useSession, useSessionValue } from '@/session';

import { askComparison, chosenFor, keepChosen, resolve, useReviewStore } from './comparison';
import { hunksKey, memoryOf, type Chosen, type HunksResult, type StoredChanges } from './data';
import { notConnected, sentence } from './errors';

export interface ChangedFiles {
  /** True until the Mac answered the first time (or could not be asked). */
  readonly loading: boolean;
  /** True while a pull to refresh waits for its answer. */
  readonly refreshing: boolean;
  /** What the Mac refused, in one line. */
  readonly error: string | null;
  readonly choices: readonly review.ComparisonChoice[];
  readonly current: review.ComparisonChoice | null;
  readonly branches: readonly string[];
  readonly workspaceId: string | null;
  /** `null` until they are known. */
  readonly changes: readonly Change[] | null;
  readonly conflicted: readonly string[];
  /** When the Mac said so. */
  readonly at: number | null;
  choose(chosen: Chosen): void;
  /** Asks the Mac again. */
  refresh(): Promise<void>;
  /** The same, for a pull to refresh. */
  pull(): Promise<void>;
}

/** The changed files of a run against the comparison chosen for it, which is kept by run on the phone. */
export function useChangedFiles(runId: string): ChangedFiles {
  const session = useSession();
  const kept = useReviewStore();
  const memory = memoryOf(session);
  const online = useSessionValue((s) => s.connection === 'online');
  const [stored, setStored] = useState<StoredChanges | null>(() => memory.changes.get(runId) ?? null);
  const [error, setError] = useState<string | null>(null);
  // False until the Mac answered, or could not: nothing waits while it cannot be reached.
  const [settled, setSettled] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const loading = online && !settled;
  const chosen = useRef<Chosen | undefined>(undefined);
  const generation = useRef(0);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const load = useCallback(async (): Promise<void> => {
    const mine = ++generation.current;
    const current = (): boolean => alive.current && mine === generation.current;
    try {
      const resolved = await askComparison(session, runId, chosen.current);
      if (!current()) return;
      const base = resolved.current?.base ?? null;
      if (base === null) {
        setStored({ options: resolved.options, chosen: chosen.current, base: null, changes: [], conflicted: [], at: Date.now() });
        setError(text.TEXT.review.comparisonUnavailable(resolved.why || text.TEXT.review.unavailable));
        return;
      }
      const diff = await session.request('workspace.diff', { workspace_id: resolved.options.workspace.id, base, status: true });
      if (!current()) return;
      const next: StoredChanges = { options: resolved.options, chosen: chosen.current, base, changes: diff.changes, conflicted: diff.status?.conflicted ?? [], at: Date.now() };
      memory.changes.set(runId, next);
      setStored(next);
      setError(null);
    } catch (failure) {
      if (!current()) return;
      setError(notConnected(failure) ? null : sentence(failure));
    } finally {
      if (current()) {
        setSettled(true);
        setRefreshing(false);
      }
    }
  }, [session, runId, memory]);

  // Asked when the screen opens, and again each time the Mac is reached.
  useEffect(() => {
    chosen.current = chosenFor(kept, runId);
    if (runId && online) void load();
  }, [runId, online, kept, load]);

  const choose = useCallback(
    (next: Chosen) => {
      chosen.current = next;
      keepChosen(kept, runId, next);
      // What is on screen belongs to the comparison before: it goes, and the list is asked again.
      setStored((before) => (before ? { ...before, chosen: next, base: null, changes: [] } : before));
      setSettled(false);
      void load();
    },
    [kept, runId, load],
  );

  const pull = useCallback(async () => {
    setRefreshing(true);
    await load();
  }, [load]);

  return useMemo(() => {
    const resolved = stored ? resolve(stored.options, stored.chosen) : null;
    return {
      loading,
      refreshing,
      error,
      choices: resolved?.choices ?? [],
      current: resolved?.current ?? null,
      branches: stored?.options.branches ?? [],
      workspaceId: stored?.options.workspace.id ?? null,
      changes: stored && (stored.base !== null || !loading) ? stored.changes : null,
      conflicted: stored?.conflicted ?? [],
      at: stored?.at ?? null,
      choose,
      refresh: load,
      pull,
    };
  }, [stored, loading, refreshing, error, choose, load, pull]);
}

/** Asks for the hunks of files, a few at a time, each file once. */
export class HunkLoader {
  private readonly queue: string[] = [];
  private readonly asked = new Set<string>();
  private running = 0;
  private stopped = false;

  constructor(
    private readonly ask: (path: string) => Promise<HunksResult>,
    private readonly answer: (path: string, result: HunksResult) => void,
    private readonly atOnce = 4,
  ) {}

  need = (path: string): void => {
    if (this.stopped || this.asked.has(path)) return;
    this.asked.add(path);
    this.queue.push(path);
    this.pump();
  };

  stop(): void {
    this.stopped = true;
    this.queue.length = 0;
  }

  private pump(): void {
    while (!this.stopped && this.running < this.atOnce) {
      const path = this.queue.shift();
      if (path === undefined) return;
      this.running += 1;
      this.ask(path)
        .then((result) => {
          if (!this.stopped) this.answer(path, result);
        })
        .catch(() => {
          // The row keeps what it showed; the next refresh asks again.
          this.asked.delete(path);
        })
        .finally(() => {
          this.running -= 1;
          this.pump();
        });
    }
  }
}

export interface RowHunks {
  /** By path, for the files whose hunks are known and shown as text. */
  readonly hunks: Readonly<Record<string, readonly Hunk[]>>;
  /** By path, for the files that cannot be shown: why. */
  readonly notShown: Readonly<Record<string, string>>;
  /** A row that is drawn asks for its file's hunks. It changes when they have to be asked again. */
  readonly need: (path: string) => void;
}

const scopeOf = (workspaceId: string | null, base: string | null): string => `${workspaceId ?? ''}\n${base ?? ''}`;

/**
 * The hunks of the files whose rows are drawn, for their counts. `stamp` is when the changed
 * files were last answered: hunks known before stay on screen until the new ones arrive.
 */
export function useRowHunks(runId: string, workspaceId: string | null, base: string | null, stamp: number | null, paths: readonly string[]): RowHunks {
  const session = useSession();
  const memory = memoryOf(session);
  const scope = scopeOf(workspaceId, base);
  const [known, setKnown] = useState<{ readonly scope: string; readonly byPath: Readonly<Record<string, HunksResult>> }>({ scope, byPath: {} });

  const loader = useMemo(() => {
    void stamp;
    if (workspaceId === null || base === null) return null;
    return new HunkLoader(
      (path) => session.request('workspace.hunks', { workspace_id: workspaceId, path, base, run_id: runId }),
      (path, result) => {
        memory.hunks.set(hunksKey(workspaceId, base, path), result);
        setKnown((before) => ({ scope, byPath: { ...(before.scope === scope ? before.byPath : {}), [path]: result } }));
      },
    );
  }, [session, memory, runId, workspaceId, base, scope, stamp]);
  useEffect(() => () => loader?.stop(), [loader]);

  const need = useMemo(() => loader?.need ?? ((): void => undefined), [loader]);

  return useMemo(() => {
    const hunks: Record<string, readonly Hunk[]> = {};
    const notShown: Record<string, string> = {};
    for (const path of paths) {
      const result = (known.scope === scope ? known.byPath[path] : undefined) ?? (workspaceId !== null && base !== null ? memory.hunks.get(hunksKey(workspaceId, base, path)) : undefined);
      if (!result) continue;
      if (result.shown) hunks[path] = result.hunks;
      else notShown[path] = text.PHONE_ONLY.notShown(result.why ?? '');
    }
    return { hunks, notShown, need };
  }, [known, scope, paths, workspaceId, base, memory, need]);
}
