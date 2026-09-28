/** Shared test tools: deterministic randomness, stores that can fail or freeze, waiting. */

import { randomBytes } from "node:crypto";
import { WebSocket as WsWebSocket } from "ws";
import { PhoneClient } from "../src/client.ts";
import type { ClientOptions, ClientTiming, ConnectAttempt, ConnectionState, DaemonEvent } from "../src/client-types.ts";
import type { OutboxEntry } from "../src/outbox.ts";
import type { KeyValueStore, RandomSource } from "../src/platform.ts";
import { type Socket, type SocketFactory, type SocketHandlers, webSocketFactory } from "../src/socket.ts";

/** A small deterministic generator (mulberry32). Returns numbers in [0, 1). */
export function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * A deterministic `RandomSource` for tests. With `mixReal` it adds real randomness, for tests
 * that must see two different values.
 */
export function seededRandom(seed: number, mixReal = false): RandomSource {
  const next = prng(seed);
  return (n) => {
    const out = new Uint8Array(n);
    const real = mixReal ? randomBytes(n) : null;
    for (let i = 0; i < n; i++) out[i] = Math.floor(next() * 256) ^ (real ? (real[i] as number) : 0);
    return out;
  };
}

/** The platform's real random source, as the app would inject it. */
export const realRandom: RandomSource = (n) => new Uint8Array(randomBytes(n));

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Waits until `check` returns something truthy, and returns it. `what` may be made when it is needed. */
export async function waitFor<T>(check: () => T | undefined | null | false, what: string | (() => string), timeoutMs = 10_000): Promise<T> {
  const start = Date.now();
  for (;;) {
    const value = check();
    if (value) return value;
    if (Date.now() - start > timeoutMs) throw new Error(`timed out waiting for ${typeof what === "string" ? what : what()}`);
    await sleep(2);
  }
}

/** A store that records its writes in order and can be frozen, as if the app was killed. */
export class RecordingStore implements KeyValueStore {
  readonly values = new Map<string, string>();
  readonly writes: { key: string; value: string | null }[] = [];
  /** When true, writes are lost: what a killed app never got to store. */
  frozen = false;
  /** Delay of every operation, to show that ordering does not depend on a fast store. */
  delayMs = 0;

  private async pause(): Promise<void> {
    if (this.delayMs > 0) await sleep(this.delayMs);
    else await Promise.resolve();
  }

  async get(key: string): Promise<string | null> {
    await this.pause();
    return this.values.get(key) ?? null;
  }

  async set(key: string, value: string): Promise<void> {
    await this.pause();
    if (this.frozen) return;
    this.values.set(key, value);
    this.writes.push({ key, value });
  }

  async delete(key: string): Promise<void> {
    await this.pause();
    if (this.frozen) return;
    this.values.delete(key);
    this.writes.push({ key, value: null });
  }

  keys(): string[] {
    return [...this.values.keys()].sort();
  }
}

/** Short waiting times, so that tests of reconnecting run in milliseconds. */
export const FAST: Partial<ClientTiming> = {
  backoffMinMs: 4,
  backoffMaxMs: 30,
  offRetryMs: 25,
  openTimeoutMs: 1_000,
  handshakeTimeoutMs: 2_000,
  pairingTimeoutMs: 3_000,
  requestTimeoutMs: 3_000,
  probeTimeoutMs: 200,
  contactSaveMs: 0,
};

/** Sockets of Node 24's global `WebSocket`: what the app uses on React Native, by the same adapter. */
export const nodeSockets: SocketFactory = webSocketFactory(WebSocket);

/** Sockets of the `ws` package, which can also send a WebSocket ping. */
export const wsSockets: SocketFactory = webSocketFactory(WsWebSocket);

/** One connection as a recording factory saw it. */
export interface RecordedConnection {
  readonly url: string;
  /** Frames the client sent, in order. The first is the handshake. */
  readonly sent: Uint8Array[];
  /** Frames the client received, in order. */
  readonly received: Uint8Array[];
  opened: boolean;
  closed: boolean;
  pings: number;
}

/** A socket factory that records what passes, and can stand for a network that is down. */
export class RecordingSockets {
  readonly connections: RecordedConnection[] = [];
  /** While true, nothing can be reached. */
  down = false;
  private readonly inner: SocketFactory;
  private readonly live = new Set<Socket>();

  constructor(inner: SocketFactory = nodeSockets) {
    this.inner = inner;
  }

  /** First frames of the given kind (1 session, 2 pairing) that were sent. */
  handshakes(kind: number): number {
    return this.connections.filter((c) => c.sent.length > 0 && (c.sent[0] as Uint8Array)[1] === kind).length;
  }

  /** Cuts every connection, as a network that went away. */
  cutAll(): void {
    for (const socket of [...this.live]) socket.close();
  }

  readonly factory: SocketFactory = (url: string, handlers: SocketHandlers): Socket => {
    const record: RecordedConnection = { url, sent: [], received: [], opened: false, closed: false, pings: 0 };
    this.connections.push(record);
    if (this.down) {
      void Promise.resolve().then(() => {
        record.closed = true;
        handlers.onError(new Error("the network is down"));
        handlers.onClose({ code: null, reason: "" });
      });
      return { send: () => undefined, close: () => undefined };
    }
    const socket = this.inner(url, {
      onOpen: () => {
        record.opened = true;
        handlers.onOpen();
      },
      onMessage: (data) => {
        record.received.push(data.slice());
        handlers.onMessage(data);
      },
      onError: (error) => handlers.onError(error),
      onClose: (info) => {
        record.closed = true;
        this.live.delete(wrapped);
        handlers.onClose(info);
      },
    });
    const wrapped: Socket = {
      send: (data) => {
        record.sent.push(data.slice());
        socket.send(data);
      },
      close: (code, reason) => socket.close(code, reason),
    };
    if (socket.ping) {
      const ping = socket.ping.bind(socket);
      wrapped.ping = () => {
        record.pings += 1;
        ping();
      };
    }
    this.live.add(wrapped);
    return wrapped;
  };
}

/** Everything a phone keeps: what survives when the app is closed and opened again. */
export interface Phone {
  readonly store: RecordingStore;
  readonly secrets: RecordingStore;
  readonly sockets: RecordingSockets;
}

export function newPhone(inner: SocketFactory = nodeSockets): Phone {
  return { store: new RecordingStore(), secrets: new RecordingStore(), sockets: new RecordingSockets(inner) };
}

/** A client with everything it reported, for assertions. */
export interface Harness {
  readonly client: PhoneClient;
  readonly phone: Phone;
  readonly states: ConnectionState[];
  /** The `seq` of every event delivered, in order. */
  readonly seqs: number[];
  readonly events: DaemonEvent[];
  readonly attempts: ConnectAttempt[];
  readonly outbox: OutboxEntry[];
  readonly truncated: number[];
  readonly replayed: number[];
  /** For every event delivered, whether it was news (true) or replayed history (false). */
  readonly live: boolean[];
  readonly logs: string[];
  /** How often the pairing path ran. */
  pairCalls: number;
}

/** A client on `phone`, as the app makes one at every start. */
export function newClient(phone: Phone, overrides: Partial<ClientOptions> = {}, shared?: { seqs?: number[] }): Harness {
  const logs: string[] = [];
  const client = new PhoneClient({
    socketFactory: phone.sockets.factory,
    store: phone.store,
    secrets: phone.secrets,
    random: realRandom,
    now: Date.now,
    app: "0.1.0",
    log: (line) => logs.push(line),
    timing: FAST,
    ...overrides,
  });
  const harness: Harness = { client, phone, states: [], seqs: shared?.seqs ?? [], events: [], attempts: [], outbox: [], truncated: [], replayed: [], live: [], logs, pairCalls: 0 };
  const pair = client.pair.bind(client);
  client.pair = (code, name, platform) => {
    harness.pairCalls += 1;
    return pair(code, name, platform);
  };
  client.on("state", (state) => void harness.states.push(state));
  client.on("event", (event, info) => {
    harness.seqs.push(event.seq);
    harness.events.push(event);
    harness.live.push(info.live);
  });
  client.on("attempt", (attempt) => void harness.attempts.push(attempt));
  client.on("outbox", (entry) => void harness.outbox.push(entry));
  client.on("truncated", (info) => void harness.truncated.push(info.cursor));
  client.on("replayed", (cursor) => void harness.replayed.push(cursor));
  return harness;
}

/** Waits until the client is in `state`. */
export function untilState(harness: Harness, state: ConnectionState, timeoutMs = 10_000): Promise<unknown> {
  return waitFor(
    () => harness.client.state === state,
    () => `the state ${state} (it is ${harness.client.state}; it was ${harness.states.join(", ")}; tried ${harness.attempts.map((a) => `${a.address.port} ${a.outcome}`).join(", ")})`,
    timeoutMs,
  );
}

/** Waits until the client has entered `state` at least `times` times since it was made. */
export function untilEntered(harness: Harness, state: ConnectionState, times = 1, timeoutMs = 10_000): Promise<unknown> {
  return waitFor(
    () => harness.states.filter((s) => s === state).length >= times,
    () => `${times} times ${state} (it was ${harness.states.join(", ")})`,
    timeoutMs,
  );
}
