import { createContext, useContext, useMemo, useSyncExternalStore, type ReactNode } from 'react';

import type { Session } from './session';
import type { ConversationSnapshot, SessionSnapshot } from './types';

const SessionContext = createContext<Session | null>(null);

export function SessionProvider({ session, children }: { session: Session; children: ReactNode }) {
  return <SessionContext.Provider value={session}>{children}</SessionContext.Provider>;
}

/** The session itself, to send a request or to pair. To read, use `useSessionValue`. */
export function useSession(): Session {
  const session = useContext(SessionContext);
  if (!session) throw new Error('useSession needs a SessionProvider above it');
  return session;
}

/**
 * One value of the session's snapshot. The component draws again only when that value changes:
 * `select` must return the same reference for the same content (a field of the snapshot, or a
 * value the model keeps the same until it changes).
 */
export function useSessionValue<T>(select: (snapshot: SessionSnapshot) => T): T {
  const session = useSession();
  const get = (): T => select(session.getSnapshot());
  return useSyncExternalStore(session.subscribe, get, get);
}

/** An open conversation, live. It is loaded the first time it is asked for. */
export function useConversation(runId: string): ConversationSnapshot {
  const session = useSession();
  const handle = useMemo(() => session.conversation(runId), [session, runId]);
  return useSyncExternalStore(handle.subscribe, handle.getSnapshot, handle.getSnapshot);
}
