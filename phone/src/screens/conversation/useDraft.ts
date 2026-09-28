import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { useCapabilities, type SyncStore } from '@/platform';

import { storeKey } from './ids';

type Drafts = SyncStore<Record<string, string>>;

const WRITE_AFTER_MS = 400;

/** What was typed to an agent and not sent, kept on the phone for each agent. */
export function useDraft(runId: string): readonly [string, (text: string) => void] {
  const { keyValue } = useCapabilities();
  const key = storeKey(runId);
  const drafts = useMemo<Drafts | null>(() => {
    try {
      return keyValue.scope<Record<string, string>>('drafts');
    } catch {
      return null;
    }
  }, [keyValue]);
  const [text, setText] = useState(() => readDraft(drafts, key));
  const latest = useRef(text);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const write = useCallback(() => {
    timer.current = null;
    try {
      if (latest.current) drafts?.set(key, latest.current);
      else drafts?.delete(key);
    } catch {
      // A draft that cannot be stored is still on screen.
    }
  }, [drafts, key]);

  const change = useCallback(
    (next: string) => {
      latest.current = next;
      setText(next);
      if (timer.current !== null) clearTimeout(timer.current);
      // Emptied by sending: forgotten at once, so a message is never offered again as a draft.
      if (next === '') write();
      else timer.current = setTimeout(write, WRITE_AFTER_MS);
    },
    [write],
  );

  useEffect(
    () => () => {
      if (timer.current !== null) {
        clearTimeout(timer.current);
        write();
      }
    },
    [write],
  );

  return [text, change];
}

function readDraft(drafts: Drafts | null, key: string): string {
  try {
    const stored = drafts?.get(key);
    return typeof stored === 'string' ? stored : '';
  } catch {
    return '';
  }
}
