/** The public types of the client: its states, options, events and what it reports. */

import type { Event as ProtocolEvent } from "../../protocol/protocol.generated.ts";
import type { Address, AddressInput } from "./addresses.ts";
import type { JsonValue } from "./json.ts";
import type { HandshakeKind } from "./noise.ts";
import type { OutboxEntry } from "./outbox.ts";
import type { Clock, KeyValueStore, Log, RandomSource, SecretStore, Timers } from "./platform.ts";
import { type ConnectFailureCode, DEFAULT_SESSION_TIMING, type SessionTiming } from "./session.ts";
import type { SocketFactory } from "./socket.ts";

/** The states of the connection. There are exactly these seven. */
export const CONNECTION_STATES = ["unpaired", "connecting", "online", "reconnecting", "off", "unreachable", "revoked"] as const;

/**
 * - `unpaired`: there is no pairing; the app offers pairing.
 * - `connecting`: paired, and the first connection since the start is being made.
 * - `online`: a session is up and `hello` was answered.
 * - `reconnecting`: the session was lost and a new one is being made.
 * - `off`: the gateway said phone access was turned off. Retries go on quietly.
 * - `unreachable`: a whole pass over every address failed. Retries go on.
 * - `revoked`: the gateway said this device was revoked. The pairing is being forgotten;
 *   `unpaired` follows.
 */
export type ConnectionState = (typeof CONNECTION_STATES)[number];

/** Waiting times of the client, in milliseconds. */
export interface ClientTiming extends SessionTiming {
  /** The first wait between reconnection passes. It doubles up to `backoffMaxMs`. */
  readonly backoffMinMs: number;
  readonly backoffMaxMs: number;
  /** The wait between quiet retries while phone access is off. */
  readonly offRetryMs: number;
  /** How long a reply to a request that is not a control request may take. */
  readonly requestTimeoutMs: number;
  /** After `wake()` on a live session, a sign of life must come within this time. */
  readonly probeTimeoutMs: number;
  /**
   * A control request that was sent and never answered is sent again only within this time of
   * its first sending. The gateway keeps outcomes for at least 24 hours; after that a retry
   * could run the action twice, so the entry fails with `outcome_unknown` instead.
   */
  readonly retryWindowMs: number;
  /** `lastContact` is stored at most this often while frames keep arriving. */
  readonly contactSaveMs: number;
}

export const DEFAULT_CLIENT_TIMING: ClientTiming = {
  ...DEFAULT_SESSION_TIMING,
  backoffMinMs: 500,
  backoffMaxMs: 10_000,
  offRetryMs: 10_000,
  requestTimeoutMs: 30_000,
  probeTimeoutMs: 3_000,
  retryWindowMs: 24 * 60 * 60 * 1000,
  contactSaveMs: 5_000,
};

/** What the app injects. */
export interface ClientOptions {
  /** Opens sockets. `webSocketFactory(WebSocket)` on React Native and Node 24. */
  readonly socketFactory: SocketFactory;
  /** Durable storage for everything that is not a key. */
  readonly store: KeyValueStore;
  /** The system keystore, for the device's keys and the gateway's key. */
  readonly secrets: SecretStore;
  /** The platform's secure random generator. */
  readonly random: RandomSource;
  readonly now: Clock;
  /** The app's version, sent in every handshake. */
  readonly app: string;
  readonly timers?: Timers;
  readonly log?: Log;
  /** Addresses the platform adds, tried last. */
  readonly extras?: readonly AddressInput[];
  /** The start of every storage key. The default is `overseer.`. */
  readonly namespace?: string;
  /**
   * Called when the pairing was forgotten (revoked on the Mac, or removed in the app), after
   * the keys were deleted from the `SecretStore`, for anything else the app must delete.
   */
  readonly onForget?: (reason: ForgetReason) => void | Promise<void>;
  readonly timing?: Partial<ClientTiming>;
}

export type ForgetReason = "revoked" | "forgotten";

/** Options of one request. */
export interface RequestOptions {
  /**
   * How long to wait for the reply. For a control request this only ends the waiting: the
   * request stays in the outbox and is sent until it is answered.
   */
  readonly timeoutMs?: number;
  /** The request id of an action that is tried again. The default is a new id. */
  readonly requestId?: string;
}

/** Options of a request made by name, without the protocol's types. */
export interface RawRequestOptions extends RequestOptions {
  /**
   * For a method the protocol description does not know yet: true when it changes something,
   * so that it carries a request id and goes through the outbox. For a method the description
   * knows, its class decides and this is ignored.
   */
  readonly control?: boolean;
}

/** An event of the daemon, as the protocol describes it. `seq` orders them. */
export type DaemonEvent = ProtocolEvent;

/** What came of trying one address. */
export interface ConnectAttempt {
  readonly address: Address;
  readonly url: string;
  readonly kind: HandshakeKind;
  /** `connected`, or how it failed. `hello_failed` means the session came up and `hello` was not answered. */
  readonly outcome: "connected" | "hello_failed" | ConnectFailureCode;
  readonly at: number;
}

/** The paired gateway and this device, as far as the app may show them. */
export interface GatewayInfo {
  readonly deviceId: string;
  readonly deviceName: string;
  readonly platform: string;
  readonly gatewayName: string;
  /** The fingerprint of the gateway's key: the first 16 hex characters of its SHA-256. */
  readonly gatewayFingerprint: string;
  /** `full` or `watch`. */
  readonly scope: string;
  readonly pairedAt: number;
}

/** What the client knows about an event beyond its content. */
export interface EventInfo {
  /**
   * False while the daemon replays what was missed: the event is history, not news, and
   * nothing should ring or buzz for it. True after `replayed`.
   */
  readonly live: boolean;
}

/** The events of the client. */
export interface ClientEvents {
  state: (state: ConnectionState, previous: ConnectionState) => void;
  /**
   * One event of the daemon, exactly once and in order. The next event waits until the
   * promise of every listener resolved; the cursor moves after that.
   */
  event: (event: DaemonEvent, info: EventInfo) => void | Promise<void>;
  /** The daemon finished replaying what was missed; what follows is live. */
  replayed: (cursor: number) => void;
  /** The daemon no longer has every event after the cursor: reload `state` and say so. */
  truncated: (info: { readonly cursor: number }) => void;
  /** An outbox entry was added or changed its state. */
  outbox: (entry: OutboxEntry) => void;
  /** One address was tried. */
  attempt: (attempt: ConnectAttempt) => void;
  /** A notification this library does not handle itself. */
  notification: (method: string, params: JsonValue) => void;
  /** Pairing succeeded. */
  paired: (gateway: GatewayInfo) => void;
  /** The pairing was forgotten. */
  forgotten: (reason: ForgetReason) => void;
}
