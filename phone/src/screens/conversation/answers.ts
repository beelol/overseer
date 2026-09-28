import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { record, type OutboxEntry } from '@/model';
import { useSession } from '@/session';

/** What this phone knows of the answer to a permission request before the daemon's event says it. */
export interface Answer {
  readonly allow: boolean;
  /** Who answered, when it was somebody else first ("the Mac"); `null` for this phone's own answer. */
  readonly by: string | null;
  /** sending: on its way. answered: the Mac took it, or had an answer already. failed: not taken. */
  readonly state: 'sending' | 'answered' | 'failed';
}

/** The first answer, from the daemon's `already_answered` error: its data has `allow` and `by`. */
function firstAnswer(error: unknown): Answer | undefined {
  const e = record(error);
  if (e['code'] !== 'already_answered') return undefined;
  const data = record(e['data']);
  if (typeof data['allow'] !== 'boolean') return undefined;
  return { allow: data['allow'], by: typeof data['by'] === 'string' ? data['by'] : null, state: 'answered' };
}

/** The answers the outbox holds for this agent's requests, the latest for each request. */
export function answersIn(outbox: readonly OutboxEntry[], runId: string): ReadonlyMap<string, Answer> {
  const out = new Map<string, Answer>();
  for (const entry of [...outbox].sort((a, b) => a.createdAt - b.createdAt)) {
    if (entry.method !== 'run.permission') continue;
    const p = record(entry.params);
    if (p['run_id'] !== runId || typeof p['request_id'] !== 'string') continue;
    const allow = p['allow'] === true;
    if (entry.state === 'failed') out.set(p['request_id'], firstAnswer(record(entry)['error']) ?? { allow, by: null, state: 'failed' });
    else out.set(p['request_id'], { allow, by: null, state: entry.state === 'done' ? 'answered' : 'sending' });
  }
  return out;
}

const NONE: ReadonlyMap<string, Answer> = new Map();

/**
 * Answers to this agent's permission requests. The choice shows at once; when the Mac says the
 * request was answered already, the answer becomes what was answered first, and by whom.
 */
export function useAnswers(runId: string, outbox: readonly OutboxEntry[]): { readonly answers: ReadonlyMap<string, Answer>; readonly answer: (requestId: string, allow: boolean, message: string) => void } {
  const session = useSession();
  const [own, setOwn] = useState<ReadonlyMap<string, Answer>>(NONE);
  const here = useRef(true);
  useEffect(() => {
    here.current = true;
    return () => {
      here.current = false;
    };
  }, []);

  const answer = useCallback(
    (requestId: string, allow: boolean, message: string) => {
      const put = (value: Answer): void => {
        if (here.current) setOwn((before) => new Map(before).set(requestId, value));
      };
      put({ allow, by: null, state: 'sending' });
      const sentence = message.trim();
      session.request('run.permission', { run_id: runId, request_id: requestId, allow, ...(sentence ? { message: sentence } : {}) }).then(
        () => put({ allow, by: null, state: 'answered' }),
        (error: unknown) => put(firstAnswer(error) ?? { allow, by: null, state: 'failed' }),
      );
    },
    [session, runId],
  );

  const queued = useMemo(() => answersIn(outbox, runId), [outbox, runId]);
  const answers = useMemo(() => {
    if (own.size === 0) return queued.size === 0 ? NONE : queued;
    const all = new Map(queued);
    // What the request's own reply said is the newest word on it.
    for (const [id, value] of own) if (value.state !== 'sending' || !all.has(id)) all.set(id, value);
    return all;
  }, [own, queued]);

  return { answers, answer };
}
