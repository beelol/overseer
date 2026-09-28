import { useEffect, useMemo, useRef } from 'react';

import { conversation as conversations, store } from '@/model';
import { useConversation, useSessionValue } from '@/session';

/** What happened in a run that the review depends on, as counts: a count that grew is news. */
export interface Activity {
  /** False until the run's history has arrived: what it holds is not news. */
  readonly ready: boolean;
  /** Turns started: the comparison "Latest run" moves with each. */
  readonly starts: number;
  /** Edits the agent made. */
  readonly edits: number;
  /** Turns that ended. */
  readonly ends: number;
  /** Hunks marked or unmarked, here or on the Mac. */
  readonly marks: number;
  /** Hunks put back, here or on the Mac. */
  readonly rejects: number;
  /** Steps of merging back. */
  readonly merges: number;
  readonly status: string | undefined;
}

// An event the conversation has no row of its own for becomes a quiet line that says its kind
// (as VS Code's feed does); that is how these reach a screen.
const said = (kind: string): string => kind.replace(/_/g, ' ');
const MARK = said('review_mark');
const REJECT = said('review_reject');
const MERGE = said('merge_back');

function count(conversation: conversations.Conversation): Omit<Activity, 'ready' | 'status'> {
  let starts = 0;
  let edits = 0;
  let ends = 0;
  let marks = 0;
  let rejects = 0;
  let merges = 0;
  for (const row of conversations.rowsOf(conversation)) {
    switch (row.kind) {
      case 'user':
        starts += 1;
        break;
      case 'edit':
        edits += 1;
        break;
      case 'footer':
        if (row.state !== null) ends += 1;
        break;
      case 'note':
        if (row.text === MARK) marks += 1;
        else if (row.text === REJECT) rejects += 1;
        else if (row.text === MERGE) merges += 1;
        break;
      default:
        break;
    }
  }
  return { starts, edits, ends, marks, rejects, merges };
}

/** The run's edits, turn ends and review marks, from the conversation and the state the app already keeps. */
export function useActivity(runId: string): Activity {
  const snapshot = useConversation(runId);
  const status = useSessionValue((s) => store.run(s.state, runId)?.status);
  const counts = useMemo(() => count(snapshot.conversation), [snapshot.conversation]);
  return useMemo(() => ({ ...counts, status, ready: !snapshot.loading }), [counts, status, snapshot.loading]);
}

/** The changes are asked again this long after the last edit, */
export const REFRESH_AFTER_MS = 500;
/** and no later than this after the first, however fast the agent edits. */
export const REFRESH_AT_MOST_MS = 2000;

/**
 * Runs `refresh` after `signal` changed, once the run's history has arrived. Changes that
 * follow each other closely are one refresh.
 */
export function useRefreshOn(signal: string, ready: boolean, refresh: () => void): void {
  const seen = useRef<string | null>(null);
  const waitingSince = useRef<number | null>(null);
  const latest = useRef(refresh);
  useEffect(() => {
    latest.current = refresh;
  }, [refresh]);
  useEffect(() => {
    if (!ready) return undefined;
    if (seen.current === null || seen.current === signal) {
      seen.current = signal;
      return undefined;
    }
    seen.current = signal;
    const now = Date.now();
    waitingSince.current ??= now;
    const wait = Math.max(0, Math.min(REFRESH_AFTER_MS, waitingSince.current + REFRESH_AT_MOST_MS - now));
    const timer = setTimeout(() => {
      waitingSince.current = null;
      latest.current();
    }, wait);
    return () => clearTimeout(timer);
  }, [signal, ready]);
}
