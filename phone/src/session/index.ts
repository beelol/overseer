/**
 * The app's connection to Overseer. One `Session` holds the phone's copy of the daemon's state
 * and the open conversations; screens read it through the hooks and send requests through it.
 */
export { SessionProvider, useConversation, useSession, useSessionValue } from './context';
export { createSession } from './create';
export { Session, type SessionCache, type SessionDeps } from './session';
export type { Connection, ConversationSnapshot, NotificationSwitches, Scope, SessionSnapshot } from './types';
