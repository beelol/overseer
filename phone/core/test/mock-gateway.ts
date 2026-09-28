/**
 * A gateway for tests, in TypeScript: a WebSocket server that is the responder of both
 * handshakes, keeps the counter rule, pairs with a confirm hook, and holds a tiny daemon
 * (`hello`, `ping`, `state`, `events.subscribe`, one control method that counts how often it
 * really ran, and the store that makes it run once per request id).
 *
 * It follows docs/rfcs/phone-remote-protocol.md and what daemon/src/gateway does, so that the
 * client is tested against the behaviour of the real gateway: a peer that is not welcome gets
 * the connection closed and nothing else.
 */

import type { AddressInfo } from "node:net";
import { type RawData, type WebSocket as ServerSocket, WebSocketServer } from "ws";
import { bytesToHex, utf8Decode, utf8Encode } from "../src/bytes.ts";
import type { DaemonEvent } from "../src/client-types.ts";
import { MAX_REQUEST_BYTES, Opener, seal } from "../src/frames.ts";
import { isJsonObject, type JsonObject, type JsonValue } from "../src/json.ts";
import { type CipherState, fingerprint, generateKeyPair, Handshake, type KeyPair, pskFromSecret } from "../src/noise.ts";
import { encodePairingCode } from "../src/pairing-code.ts";
import type { Clock, RandomSource } from "../src/platform.ts";
import { FRAME_VERSION, KIND_PAIRING, KIND_SESSION, MAX_FIRST_FRAME } from "../src/session.ts";
import { realRandom, sleep } from "./helpers.ts";

export interface MockDevice {
  readonly id: string;
  name: string;
  platform: string;
  app: string;
  /** Hex of the device's static public key. */
  readonly publicKey: string;
  scope: "full" | "watch";
  lastCounter: number;
  revoked: boolean;
}

/** What the owner is asked to confirm. */
export interface PairingRequest {
  readonly name: string;
  readonly platform: string;
  readonly fingerprint: string;
}

export type ConfirmHook = (request: PairingRequest) => boolean | Promise<boolean>;

export interface MockGatewayOptions {
  readonly name?: string;
  readonly keys?: KeyPair;
  readonly random?: RandomSource;
  readonly now?: Clock;
  /**
   * An impostor that tries anyway: a handshake it cannot read is answered with bytes of its
   * own. The real gateway never does this; it closes the connection.
   */
  readonly answersWhatItCannotRead?: boolean;
  /** How long the control method takes. */
  readonly controlDelayMs?: number;
  /** The protocol version the gateway states in its answer. The default is 1. */
  readonly protocol?: number;
  /** How long sessions get to receive a notice before they are closed. */
  readonly noticeGraceMs?: number;
}

/** One request as the gateway received it. */
export interface ReceivedRequest {
  readonly device: string;
  readonly id: JsonValue;
  readonly method: string;
  readonly params: JsonObject;
  readonly requestId: string | null;
}

interface Subscription {
  cursor: number;
  live: boolean;
}

interface Connection {
  readonly ws: ServerSocket;
  transport: { send: CipherState; receive: CipherState } | null;
  readonly opener: Opener;
  device: MockDevice | null;
  subscriptions: Subscription[];
  closed: boolean;
  first: boolean;
}

interface OpenPairing {
  readonly secret: Uint8Array;
  readonly psk: Uint8Array;
  readonly openedAt: number;
  readonly confirm: ConfirmHook;
  failures: number;
  used: boolean;
}

/** The methods of the tiny daemon, with their classes as in protocol/protocol.json. */
const CLASSES: Readonly<Record<string, "self" | "read" | "control" | "mac_only">> = {
  hello: "self",
  ping: "self",
  "events.subscribe": "self",
  state: "read",
  "run.follow_up": "control",
  "run.permission": "control",
  "daemon.shutdown": "mac_only",
};

/** The outcome of a request id: its reply, or that the daemon stopped while it ran. */
type Outcome = { method: string; reply: Promise<JsonObject> | "interrupted" };

const PAIRING_WINDOW_MS = 120_000;
const MAX_PAIRING_FAILURES = 5;

function toBytes(data: RawData): Uint8Array {
  if (Array.isArray(data)) return new Uint8Array(Buffer.concat(data));
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  return new Uint8Array(data.buffer, data.byteOffset, data.byteLength).slice();
}

export class MockGateway {
  readonly name: string;
  readonly keys: KeyPair;
  readonly devices = new Map<string, MockDevice>();
  /** The daemon's event log. It only grows, except by `truncateBefore`. */
  readonly events: DaemonEvent[] = [];
  /** Every request received inside a session, in order. */
  readonly received: ReceivedRequest[] = [];
  /** Why connections were refused, in order. */
  readonly refusals: string[] = [];
  /** How often the control method really ran. */
  executions = 0;
  /** How often the control method started and the daemon stopped before it ended. */
  interrupted = 0;
  /** First frames of kind pairing that arrived. */
  pairingHandshakes = 0;
  /** First frames of kind session that arrived. */
  sessionHandshakes = 0;
  /** How often the owner was asked to confirm a pairing. */
  confirmCalls = 0;
  /** Handshakes answered with message 2. */
  accepted = 0;
  port = 0;

  private readonly random: RandomSource;
  private readonly now: Clock;
  private readonly options: MockGatewayOptions;
  private server: WebSocketServer | null = null;
  private readonly connections = new Set<Connection>();
  private pairing: OpenPairing | null = null;
  private readonly outcomes = new Map<string, Outcome>();
  private readonly answers = new Map<string, { allow: boolean; by: string; ts: number }>();
  private stopWhileRunning = 0;
  private lastSeq = 0;
  private nextDevice = 1;
  private cutAfter: number | null = null;
  private cutReplies = 0;
  private cutReplyMethod: string | null = null;

  constructor(options: MockGatewayOptions = {}) {
    this.options = options;
    this.name = options.name ?? "Test Mac";
    this.random = options.random ?? realRandom;
    this.now = options.now ?? Date.now;
    this.keys = options.keys ?? generateKeyPair(this.random);
  }

  get fingerprint(): string {
    return fingerprint(this.keys.publicKey);
  }

  get listening(): boolean {
    return this.server !== null;
  }

  /** Sessions that completed the handshake and are open. */
  get sessions(): number {
    let n = 0;
    for (const c of this.connections) if (c.transport && !c.closed) n++;
    return n;
  }

  /** Sessions with a live subscription: everything missed was replayed to them. */
  get liveSubscribers(): number {
    let n = 0;
    for (const c of this.connections) if (!c.closed && c.subscriptions.some((s) => s.live)) n++;
    return n;
  }

  // ------------------------------------------------------------------ on and off

  /** Listens on `port` (0 picks a free one) of the loopback address. */
  async start(port: number = this.port): Promise<number> {
    if (this.server) return this.port;
    const server = new WebSocketServer({ host: "127.0.0.1", port, path: "/v1", maxPayload: 70_000 });
    await new Promise<void>((resolve, reject) => {
      server.once("listening", resolve);
      server.once("error", reject);
    });
    server.on("connection", (ws) => this.accept(ws));
    this.server = server;
    this.port = (server.address() as AddressInfo).port;
    return this.port;
  }

  /** Stops at once, as a daemon that was killed: every connection is cut, nothing is said. */
  async stop(): Promise<void> {
    const server = this.server;
    this.server = null;
    this.dropAll();
    if (server) await new Promise<void>((resolve) => server.close(() => resolve()));
  }

  /** The same gateway (keys, devices, log) listening somewhere else: the Mac's address changed. */
  async moveTo(port: number): Promise<number> {
    await this.stop();
    return this.start(port);
  }

  /** Phone access is turned off: every session hears it, then the gateway stops listening. */
  async turnOff(): Promise<void> {
    await this.endSessions(() => true, "off");
    await this.stop();
  }

  /** Phone access is turned on again, on the same port, with the same key and devices. */
  async turnOn(): Promise<void> {
    await this.start(this.port);
  }

  /** The owner revokes a device: it hears it, its session ends, its key never works again. */
  async revoke(deviceId: string): Promise<void> {
    const device = this.devices.get(deviceId);
    if (!device) throw new Error(`no device ${deviceId}`);
    device.revoked = true;
    await this.endSessions((c) => c.device === device, "revoked");
  }

  private async endSessions(which: (c: Connection) => boolean, state: string): Promise<void> {
    const targets = [...this.connections].filter((c) => c.transport && !c.closed && which(c));
    for (const c of targets) this.send(c, { method: "gateway", params: { state } });
    await sleep(this.options.noticeGraceMs ?? 20);
    for (const c of targets) this.cut(c);
  }

  // ------------------------------------------------------------------ faults on demand

  /** Cuts every connection now, without a word. */
  dropAll(): void {
    for (const c of [...this.connections]) this.cut(c);
  }

  /** Cuts the connection that is about to send its `n + 1`th frame from now, instead of sending it. */
  cutAfterFrames(n: number): void {
    this.cutAfter = n;
  }

  /** True while `cutAfterFrames` waits for its frame. */
  get cutArmed(): boolean {
    return this.cutAfter !== null;
  }

  /**
   * The next `times` control requests are received and run, and the connection is cut before
   * their reply is sent.
   */
  cutBeforeReply(times = 1, method: string | null = null): void {
    this.cutReplies = times;
    this.cutReplyMethod = method;
  }

  /**
   * The Mac stops while the next `times` control requests run: each is claimed, never
   * finished, and its connection is cut. A retry is answered with `outcome_unknown` and is
   * not run again.
   */
  stopWhileNextRuns(times = 1): void {
    this.stopWhileRunning = times;
  }

  /** Another surface (VS Code, the terminal) answers a permission request first. */
  answerElsewhere(permission: string, allow: boolean, by: string): void {
    this.answers.set(permission, { allow, by, ts: this.now() });
  }

  /** The event stream lagged: subscribers are told to resume from their cursor. */
  resync(): void {
    for (const c of this.connections) {
      if (c.closed || c.subscriptions.length === 0) continue;
      const cursor = Math.max(...c.subscriptions.map((s) => s.cursor));
      c.subscriptions = [];
      this.send(c, { method: "resync", params: { cursor } });
    }
  }

  private cut(c: Connection): void {
    if (c.closed) return;
    c.closed = true;
    c.subscriptions = [];
    this.connections.delete(c);
    c.ws.terminate();
  }

  // ------------------------------------------------------------------ pairing

  /** Opens pairing and returns the code the phone scans or types. */
  openPairing(options: { confirm?: ConfirmHook; addresses?: readonly string[]; port?: number } = {}): string {
    const secret = this.random(16);
    this.pairing = { secret, psk: pskFromSecret(secret), openedAt: this.now(), confirm: options.confirm ?? (() => true), failures: 0, used: false };
    return encodePairingCode({ gatewayPublicKey: this.keys.publicKey, secret, port: options.port ?? this.port, addresses: options.addresses ?? ["127.0.0.1"] });
  }

  get pairingOpen(): boolean {
    const p = this.pairing;
    return p !== null && !p.used && p.failures < MAX_PAIRING_FAILURES && this.now() - p.openedAt <= PAIRING_WINDOW_MS;
  }

  // ------------------------------------------------------------------ the event log

  /** Appends an event to the log and sends it to every live subscriber. */
  emit(payload: JsonObject = {}): DaemonEvent {
    const event: DaemonEvent = { seq: ++this.lastSeq, ts: this.now(), kind: "line", source: "mock", confidence: "exact", payload };
    this.events.push(event);
    for (const c of this.connections) {
      for (const sub of c.subscriptions) {
        if (c.closed || !sub.live || event.seq <= sub.cursor) continue;
        sub.cursor = event.seq;
        this.send(c, { method: "event", params: event as unknown as JsonObject });
      }
    }
    return event;
  }

  /** The daemon no longer keeps events before `seq`. */
  truncateBefore(seq: number): void {
    while (this.events.length > 0 && (this.events[0] as DaemonEvent).seq < seq) this.events.shift();
  }

  // ------------------------------------------------------------------ connections

  private accept(ws: ServerSocket): void {
    const c: Connection = { ws, transport: null, opener: new Opener(MAX_REQUEST_BYTES), device: null, subscriptions: [], closed: false, first: true };
    this.connections.add(c);
    ws.on("message", (data, isBinary) => {
      if (c.closed) return;
      if (!isBinary) {
        this.refuse(c, "a frame that is not binary");
        return;
      }
      const frame = toBytes(data);
      if (c.first) {
        c.first = false;
        void this.handshake(c, frame);
      } else if (c.transport) {
        this.frame(c, frame);
      }
    });
    ws.on("close", () => {
      c.closed = true;
      c.subscriptions = [];
      this.connections.delete(c);
    });
    ws.on("error", () => this.cut(c));
  }

  private refuse(c: Connection, why: string): void {
    this.refusals.push(why);
    this.cut(c);
  }

  private async handshake(c: Connection, first: Uint8Array): Promise<void> {
    const kind = first[1];
    if (first.length <= 2 || first.length > MAX_FIRST_FRAME || first[0] !== FRAME_VERSION || (kind !== KIND_SESSION && kind !== KIND_PAIRING)) {
      this.refuse(c, "malformed handshake");
      return;
    }
    const message1 = first.subarray(2);
    let handshake: Handshake;
    let hello: JsonValue;
    let device: MockDevice;

    if (kind === KIND_SESSION) {
      this.sessionHandshakes += 1;
      handshake = new Handshake({ kind: "session", role: "responder", staticPrivateKey: this.keys.privateKey, random: this.random });
      try {
        hello = JSON.parse(utf8Decode(handshake.readMessage(message1))) as JsonValue;
      } catch {
        this.unreadable(c, "the handshake did not decrypt");
        return;
      }
      const key = bytesToHex(handshake.remoteStaticPublicKey as Uint8Array);
      const known = [...this.devices.values()].find((d) => d.publicKey === key && !d.revoked);
      if (!known) {
        this.refuse(c, "unknown or revoked device");
        return;
      }
      const counter = isJsonObject(hello) && typeof hello["counter"] === "number" ? hello["counter"] : 0;
      if (!(counter > known.lastCounter)) {
        this.refuse(c, "a replayed handshake");
        return;
      }
      known.lastCounter = counter;
      if (isJsonObject(hello) && typeof hello["app"] === "string") known.app = hello["app"];
      device = known;
    } else {
      this.pairingHandshakes += 1;
      const pairing = this.pairing;
      if (!pairing || !this.pairingOpen) {
        this.refuse(c, "pairing is not open");
        return;
      }
      handshake = new Handshake({ kind: "pairing", role: "responder", staticPrivateKey: this.keys.privateKey, psk: pairing.psk, random: this.random });
      try {
        hello = JSON.parse(utf8Decode(handshake.readMessage(message1))) as JsonValue;
      } catch {
        // The phone does not hold the pairing secret. The owner is never asked.
        pairing.failures += 1;
        if (pairing.failures >= MAX_PAIRING_FAILURES) this.pairing = null;
        this.unreadable(c, "a wrong pairing secret");
        return;
      }
      const key = bytesToHex(handshake.remoteStaticPublicKey as Uint8Array);
      if ([...this.devices.values()].some((d) => d.publicKey === key)) {
        this.refuse(c, "this device key was paired before");
        return;
      }
      pairing.used = true;
      const name = isJsonObject(hello) && typeof hello["name"] === "string" ? hello["name"] : "Phone";
      const platform = isJsonObject(hello) && typeof hello["platform"] === "string" ? hello["platform"] : "other";
      this.confirmCalls += 1;
      let confirmed = false;
      try {
        confirmed = await pairing.confirm({ name, platform, fingerprint: fingerprint(handshake.remoteStaticPublicKey as Uint8Array) });
      } catch {
        confirmed = false;
      }
      if (this.pairing === pairing) this.pairing = null;
      if (!confirmed || c.closed) {
        this.refuse(c, "pairing was not confirmed");
        return;
      }
      device = {
        id: `device-${this.nextDevice++}`,
        name,
        platform,
        app: isJsonObject(hello) && typeof hello["app"] === "string" ? hello["app"] : "",
        publicKey: key,
        scope: "full",
        lastCounter: isJsonObject(hello) && typeof hello["counter"] === "number" ? hello["counter"] : 0,
        revoked: false,
      };
      this.devices.set(device.id, device);
    }

    const reply = { protocol: this.options.protocol ?? 1, device: device.id, scope: device.scope, gateway: this.name, fingerprint: this.fingerprint };
    const message2 = handshake.writeMessage(utf8Encode(JSON.stringify(reply)));
    const keys = handshake.split();
    c.transport = { send: keys.send, receive: keys.receive };
    c.device = device;
    this.accepted += 1;
    c.ws.send(message2);
    if (device.revoked || !this.server) this.cut(c);
  }

  private unreadable(c: Connection, why: string): void {
    if (!this.options.answersWhatItCannotRead) {
      this.refuse(c, why);
      return;
    }
    this.refusals.push(why);
    // As long as a genuine message 2, and nothing the device can verify.
    c.ws.send(this.random(146));
  }

  private frame(c: Connection, frame: Uint8Array): void {
    const transport = c.transport;
    if (!transport) return;
    let message: Uint8Array | null;
    try {
      message = c.opener.open(transport.receive, frame);
    } catch {
      this.refuse(c, "a frame that did not decrypt");
      return;
    }
    if (message === null) return;
    void this.request(c, message);
  }

  private send(c: Connection, value: JsonValue): void {
    const transport = c.transport;
    if (!transport || c.closed) return;
    for (const frame of seal(transport.send, utf8Encode(JSON.stringify(value)))) {
      if (this.cutAfter !== null) {
        if (this.cutAfter <= 0) {
          this.cutAfter = null;
          this.cut(c);
          return;
        }
        this.cutAfter -= 1;
      }
      c.ws.send(frame);
    }
  }

  // ------------------------------------------------------------------ the tiny daemon

  private async request(c: Connection, bytes: Uint8Array): Promise<void> {
    const device = c.device as MockDevice;
    let msg: JsonValue;
    try {
      msg = JSON.parse(utf8Decode(bytes)) as JsonValue;
    } catch {
      this.send(c, { id: null, error: { code: "parse_error", message: "not JSON" } });
      return;
    }
    if (!isJsonObject(msg)) {
      this.send(c, { id: null, error: { code: "invalid_request", message: "a request is an object" } });
      return;
    }
    const id = msg["id"] ?? null;
    const method = msg["method"];
    const answer = (body: JsonObject): void => this.send(c, { id, ...body });
    if (typeof method !== "string") {
      answer({ error: { code: "invalid_request", message: "method must be a string" } });
      return;
    }
    const params = msg["params"] ?? {};
    if (!isJsonObject(params)) {
      answer({ error: { code: "invalid_params", message: "params must be an object" } });
      return;
    }
    const requestId = typeof msg["request_id"] === "string" ? msg["request_id"] : null;
    this.received.push({ device: device.id, id, method, params, requestId });

    if (device.revoked) {
      answer({ error: { code: "revoked", message: "this phone is no longer paired" } });
      return;
    }
    const kind = CLASSES[method];
    if (kind === undefined) {
      answer({ error: { code: "unknown_method", message: `unknown method ${method}` } });
      return;
    }
    if (kind === "mac_only") {
      answer({ error: { code: "mac_only", message: "this is available on the Mac only" } });
      return;
    }
    if (kind === "control" && device.scope === "watch") {
      answer({ error: { code: "watch_only", message: "this phone may watch but not control" } });
      return;
    }
    if (method === "events.subscribe") {
      await this.subscribe(c, id, params);
      return;
    }
    if (kind !== "control") {
      answer({ result: this.read(method, device) });
      return;
    }
    if (requestId === null || !/^[A-Za-z0-9-]{8,64}$/.test(requestId)) {
      answer({ error: { code: "request_id_required", message: "a changing request from a phone carries a request id" } });
      return;
    }
    const key = `${device.id}\n${requestId}`;
    if (this.stopWhileRunning > 0 && !this.outcomes.has(key)) {
      // Claimed and started; the Mac stops before there is an outcome.
      this.stopWhileRunning -= 1;
      this.interrupted += 1;
      this.outcomes.set(key, { method, reply: "interrupted" });
      this.cut(c);
      return;
    }
    const body = await this.once(device, requestId, method, params);
    if (this.cutReplies > 0 && (this.cutReplyMethod === null || this.cutReplyMethod === method)) {
      // Received and run; the reply never leaves.
      this.cutReplies -= 1;
      this.cut(c);
      return;
    }
    answer(body);
  }

  private read(method: string, device: MockDevice): JsonValue {
    if (method === "hello") {
      return { protocol: 1, version: "mock", now_ms: this.now(), device: { id: device.id, name: device.name, scope: device.scope, platform: device.platform }, gateway: { name: this.name, fingerprint: this.fingerprint } };
    }
    if (method === "ping") return { now_ms: this.now() };
    return { cursor: this.lastSeq, turns: this.executions };
  }

  /** Runs a control request at most once per device and request id; every retry gets the first outcome. */
  private once(device: MockDevice, requestId: string, method: string, params: JsonObject): Promise<JsonObject> {
    const key = `${device.id}\n${requestId}`;
    const earlier = this.outcomes.get(key);
    if (earlier) {
      if (earlier.method !== method) return Promise.resolve({ error: { code: "request_id_reused", message: "this request id was used for another action" } });
      if (earlier.reply === "interrupted") {
        return Promise.resolve({ error: { code: "outcome_unknown", message: "the Mac stopped while this was running, so it was not run again; check the agent before sending it again" } });
      }
      return earlier.reply;
    }
    const reply = (async (): Promise<JsonObject> => {
      if (this.options.controlDelayMs) await sleep(this.options.controlDelayMs);
      if (method === "run.permission") {
        const permission = typeof params["request_id"] === "string" ? params["request_id"] : "";
        const first = this.answers.get(permission);
        // The first answer wins; a later one is refused with the first one's outcome.
        if (first) return { error: { code: "already_answered", message: "this request was answered already", data: first } };
        this.answers.set(permission, { allow: params["allow"] === true, by: `phone:${device.name}`, ts: this.now() });
        this.executions += 1;
        return { result: { ok: true } };
      }
      if (typeof params["prompt"] !== "string") return { error: { code: "failed", message: "missing string parameter prompt" } };
      this.executions += 1;
      const turn = this.executions;
      this.emit({ turn, prompt: params["prompt"], source: `phone:${device.name}` });
      return { result: { turn, prompt: params["prompt"] } };
    })();
    this.outcomes.set(key, { method, reply });
    return reply;
  }

  /** Replays what is after the cursor, says `replayed`, then streams live. As `server.rs` does. */
  private async subscribe(c: Connection, id: JsonValue, params: JsonObject): Promise<void> {
    const after = typeof params["after"] === "number" ? params["after"] : 0;
    const oldest = this.events.length > 0 ? (this.events[0] as DaemonEvent).seq : null;
    const gap = oldest !== null && oldest > after + 1 && after > 0;
    const sub: Subscription = { cursor: after, live: false };
    c.subscriptions.push(sub);
    this.send(c, { id, result: { subscribed: true, after, history_truncated: gap } });
    for (;;) {
      if (c.closed || !c.subscriptions.includes(sub)) return;
      const batch = this.events.filter((e) => e.seq > sub.cursor).slice(0, 40);
      if (batch.length === 0) break;
      for (const event of batch) {
        sub.cursor = event.seq;
        this.send(c, { method: "event", params: event as unknown as JsonObject });
        if (c.closed) return;
      }
      await new Promise<void>((resolve) => setImmediate(resolve));
    }
    this.send(c, { method: "replayed", params: { cursor: sub.cursor } });
    sub.live = true;
  }
}
