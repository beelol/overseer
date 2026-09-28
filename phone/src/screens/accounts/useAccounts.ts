import { useCallback, useEffect, useMemo, useState } from 'react';

import { store } from '@/model';
import { useSession, useSessionValue } from '@/session';

import {
  byProvider,
  readList,
  readStatus,
  readUsage,
  type Listed,
  type ProviderGroup,
  type Status,
  type Usage,
} from './accounts';

const NO_STATUS: ReadonlyMap<string, Status> = new Map<string, Status>();
const NO_USAGE: ReadonlyMap<string, Usage> = new Map<string, Usage>();

function put<T>(held: ReadonlyMap<string, T>, id: string, value: T): ReadonlyMap<string, T> {
  const next = new Map(held);
  next.set(id, value);
  return next;
}

export interface Accounts {
  /** The accounts of the daemon's state, by provider. */
  readonly groups: readonly ProviderGroup[];
  /** Sign-in state and usage by account, as the Mac answered since the screen opened. */
  readonly status: ReadonlyMap<string, Status>;
  readonly usage: ReadonlyMap<string, Usage>;
  /** True while the Mac can be asked: what is not known yet is on its way. */
  readonly checking: boolean;
  /** Asks the Mac again about every account. */
  refresh(): Promise<void>;
  /** Asks the Mac again about one account: after a sign-in. */
  refreshOne(id: string): Promise<void>;
}

/**
 * The accounts are the daemon's state; their sign-in state and usage are asked of the Mac when
 * the screen opens, when the connection comes back and when an account is added.
 */
export function useAccounts(): Accounts {
  const session = useSession();
  const table = useSessionValue((s) => s.state.profiles);
  const online = useSessionValue((s) => s.connection === 'online');
  const profiles = store.rows(table);
  const [listed, setListed] = useState<Listed | null>(null);
  const [status, setStatus] = useState<ReadonlyMap<string, Status>>(NO_STATUS);
  const [usage, setUsage] = useState<ReadonlyMap<string, Usage>>(NO_USAGE);

  const refreshOne = useCallback(
    async (id: string): Promise<void> => {
      await Promise.all([
        session
          .request('profile.status', { id })
          .then((answer) => setStatus((held) => put(held, id, readStatus(answer))))
          .catch(() => undefined),
        session
          .request('account.usage', { id })
          .then((answer) => setUsage((held) => put(held, id, readUsage(answer))))
          .catch(() => undefined),
      ]);
    },
    [session],
  );

  const refresh = useCallback(async (): Promise<void> => {
    const ids = store.rows(session.getSnapshot().state.profiles).map((profile) => profile.id);
    await Promise.all([
      session
        .request('account.list', {})
        .then((answer) => setListed(readList(answer)))
        .catch(() => undefined),
      ...ids.map(refreshOne),
    ]);
  }, [session, refreshOne]);

  const ids = profiles.map((profile) => profile.id).join('\n');
  useEffect(() => {
    if (online) void refresh();
  }, [online, ids, refresh]);

  const groups = useMemo(() => byProvider(profiles, listed), [profiles, listed]);
  return { groups, status, usage, checking: online, refresh, refreshOne };
}
