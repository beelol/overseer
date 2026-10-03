import { useCallback, useEffect, useRef, useState } from 'react';

import type { AppliedMods, Result } from '@/protocol';
import { useSession, useSessionValue } from '@/session';

import { REFRESH_AFTER_MS, REFRESH_AT_MOST_MS } from '../review/activity';
import { sentence } from '../review/errors';

type Reads = {
  key: string;
  library: Result<'mods.list'> | null;
  applied: AppliedMods | null;
  libraryError: string | null;
  appliedError: string | null;
  loading: boolean;
};
const empty = (key: string): Reads => ({
  key,
  library: null,
  applied: null,
  libraryError: null,
  appliedError: null,
  loading: false,
});

/** Read-only and screen-local: no control outbox, persisted mod body, or timer polling. */
export function useMods(runId: string) {
  const session = useSession();
  const online = useSessionValue((s) => s.connection === 'online');
  const paired = useSessionValue((s) => s.paired);
  const gateway = useSessionValue((s) => s.gateway?.gatewayFingerprint ?? '');
  const key = `${gateway}:${runId}`;
  const [reads, setReads] = useState<Reads>(() => empty(key));
  const generation = useRef(0);
  const alive = useRef(false);
  const invalidate = useCallback(() => {
    generation.current++;
  }, []);
  // Route or pairing identity changed: discard the previous private read during render,
  // before it can appear under another agent/Mac. Offline retains the same owner's facts.
  if (reads.key !== key) setReads(empty(key));
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      invalidate();
    };
  }, [invalidate]);

  const refresh = useCallback(async () => {
    const now = session.getSnapshot();
    if (!runId || !alive.current || !now.paired || now.connection !== 'online') return;
    const mine = ++generation.current;
    const current = () => {
      const snapshot = session.getSnapshot();
      return (
        alive.current &&
        mine === generation.current &&
        snapshot.paired &&
        snapshot.connection === 'online' &&
        (snapshot.gateway?.gatewayFingerprint ?? '') === gateway
      );
    };
    // Guard at commit time too: route identity is reset synchronously during render,
    // so even a reply queued before passive-effect cleanup cannot fill another target.
    const commit = (change: Partial<Reads>) =>
      setReads((before) => (before.key === key && current() ? { ...before, ...change } : before));
    commit({ libraryError: null, appliedError: null, loading: true });
    // Each half can succeed independently; a slow library read cannot hide a run's answer.
    await Promise.all([
      session.request('mods.list', {}).then(
        (library) => {
          commit({ library });
        },
        (error: unknown) => {
          commit({ libraryError: sentence(error) });
        },
      ),
      session.request('mods.why', { run_id: runId }).then(
        (applied) => {
          commit({ applied });
        },
        (error: unknown) => {
          commit({ appliedError: sentence(error) });
        },
      ),
    ]);
    commit({ loading: false });
  }, [session, key, gateway, runId]);

  useEffect(() => {
    if (online && paired && runId) void refresh();
    return invalidate;
  }, [key, online, paired, runId, refresh, invalidate]);

  useEffect(() => {
    if (!online || !paired || !runId) return undefined;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let since: number | null = null;
    const off = session.subscribeMods(runId, () => {
      const now = Date.now();
      since ??= now;
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(
        () => {
          timer = null;
          since = null;
          void refresh();
        },
        Math.max(0, Math.min(REFRESH_AFTER_MS, since + REFRESH_AT_MOST_MS - now)),
      );
    });
    return () => {
      off();
      if (timer !== null) clearTimeout(timer);
    };
  }, [session, online, paired, runId, refresh]);

  return {
    ...(paired && reads.key === key ? reads : empty(key)),
    loading: reads.key === key && reads.loading && online && paired,
    online,
    paired,
    refresh,
  };
}
