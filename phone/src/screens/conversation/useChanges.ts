import { useEffect, useMemo, useRef, useState } from 'react';

import { conversation, review } from '@/model';
import type { Result } from '@/protocol';
import { useSession } from '@/session';

const REFRESH_AFTER_MS = 600;
const LOOKED_AT = 64;

/**
 * Changes when an edit arrived or a turn ended: the newest edit and the last ended turn among
 * the last rows. It reads a fixed number of rows, however long the conversation is.
 */
export function changesSignal(c: conversation.Conversation): string {
  const count = conversation.rowCount(c);
  let edit = 0;
  let turn = '';
  for (let i = count - 1; i >= 0 && i >= count - LOOKED_AT; i--) {
    const row = conversation.rowAt(c, i);
    if (row === undefined) continue;
    if (row.kind === 'edit' && row.seq > edit) edit = row.seq;
    if (row.kind === 'footer' && row.state !== null && turn === '') turn = `${row.key}:${row.state}`;
  }
  return `${edit}|${turn}`;
}

/**
 * The changed files of a workspace since the task started, asked when the screen opens and
 * again, a moment after the last of them, when edits arrive or a turn ends.
 */
export function useChanges(workspaceId: string | undefined, signal: string, online: boolean): review.ChangesSummary | null {
  const session = useSession();
  const [changes, setChanges] = useState<Result<'workspace.changes'> | null>(null);
  const asked = useRef(false);
  useEffect(() => {
    if (!workspaceId || !online) return undefined;
    let current = true;
    const timer = setTimeout(
      () => {
        asked.current = true;
        session.request('workspace.changes', { workspace_id: workspaceId }).then(
          (result) => {
            if (current) setChanges(result);
          },
          () => undefined,
        );
      },
      asked.current ? REFRESH_AFTER_MS : 0,
    );
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [session, workspaceId, signal, online]);
  return useMemo(() => (changes ? review.changesSummary(changes) : null), [changes]);
}
