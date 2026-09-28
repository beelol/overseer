import type { ConnectionState, GatewayInfo, OutboxEntry } from '@/core';
import type { conversation, DaemonEvent, store } from '@/model';
import type { MethodName, Params, PhoneMethod, Result } from '@/protocol';

/** What the app needs of the connection. `PhoneClient` is one; tests use a fake. */
export interface Connection {
  readonly state: ConnectionState;
  readonly lastContact: number | null;
  readonly gateway: GatewayInfo | null;
  readonly hello: Readonly<Record<string, unknown>> | null;
  /** The sequence number of the last event the phone received, kept across launches. */
  readonly cursor: number;
  outbox(): readonly OutboxEntry[];
  dismiss(requestId: string): void;
  on(event: 'state', listener: (state: ConnectionState, previous: ConnectionState) => void): () => void;
  on(event: 'event', listener: (event: DaemonEvent, info: { readonly live: boolean }) => void): () => void;
  on(event: 'replayed', listener: (cursor: number) => void): () => void;
  on(event: 'truncated', listener: (info: { readonly cursor: number }) => void): () => void;
  on(event: 'outbox', listener: (entry: OutboxEntry) => void): () => void;
  on(event: 'paired', listener: (gateway: GatewayInfo) => void): () => void;
  on(event: 'forgotten', listener: (reason: 'revoked' | 'forgotten') => void): () => void;
  start(): Promise<void>;
  stop(): Promise<void>;
  wake(): void;
  pair(code: string, deviceName: string, platform: string): Promise<GatewayInfo>;
  forget(): Promise<void>;
  setDiscovered(addresses: readonly { host: string; port?: number }[]): void;
  request<M extends PhoneMethod>(method: M, params: Params<M>, options?: { timeoutMs?: number; requestId?: string }): Promise<Result<M>>;
}

export type { MethodName };

/** What this phone may do, as the Mac set it. `null` until the Mac said so. */
export type Scope = 'full' | 'watch';

/** This phone's notification switches. Off means the Mac sends nothing, not sent and hidden. */
export interface NotificationSwitches {
  readonly enabled: boolean;
  readonly show_text: boolean;
  readonly kinds: { readonly permission: boolean; readonly question: boolean; readonly failure: boolean; readonly finished: boolean };
}

/** Everything the screens read about the connection and the daemon's state, as one value. */
export interface SessionSnapshot {
  /** False until what was stored on the phone has been read (a few milliseconds after launch). */
  readonly ready: boolean;
  readonly connection: ConnectionState;
  readonly paired: boolean;
  readonly gateway: GatewayInfo | null;
  readonly scope: Scope | null;
  /** The last moment anything arrived from the Mac, in milliseconds since 1970. */
  readonly lastContact: number | null;
  /** The phone's copy of the daemon's state. */
  readonly state: store.PhoneState;
  /**
   * When the Mac last confirmed `state`: the moment it was loaded, or of the last event since.
   * While not connected the screens say how old it is and never show it as live.
   */
  readonly stateAt: number | null;
  /** True while `state` is what was stored on the phone and the Mac has not confirmed it yet. */
  readonly fromCache: boolean;
  /** The Mac no longer has every event the phone missed; the state was loaded again. */
  readonly historyLost: boolean;
  /** Changing requests that are queued, being sent, or answered and not yet dismissed. */
  readonly outbox: readonly OutboxEntry[];
  /** Whether the Mac sends notifications to phones at all (its own switch). */
  readonly macNotifications: boolean;
  /** This phone's own switches, as the Mac holds them. */
  readonly notifications: NotificationSwitches;
}

/** One open conversation. The same object until its content changes. */
export interface ConversationSnapshot {
  readonly runId: string;
  readonly conversation: conversation.Conversation;
  /** True until the run's history has arrived from the Mac (or from the phone's cache). */
  readonly loading: boolean;
  /** Set when the history could not be loaded; the conversation then holds what arrived live. */
  readonly error: string | null;
  /** Older events of this run were discarded by the Mac's retention. */
  readonly truncated: boolean;
}
