/**
 * The daemon protocol client of the phone: one paired gateway, one session at a time, and the
 * promise that nothing is lost and nothing happens twice.
 *
 * - After every connect it says `hello` as client `phone`, subscribes to events after its
 *   cursor and sends what waits in the outbox.
 * - Events reach the app exactly once and in order, across reconnects and restarts.
 * - Control requests carry a request id and live in a stored outbox until they are answered.
 * - The gateway is whoever proves the key from pairing. Addresses are only places to look.
 */

import { METHOD_CLASS, type Params, type PhoneMethod, type Result } from "../../protocol/protocol.generated.ts";
import { type Address, type AddressInput, addressKey, candidateAddresses, formatAddress, gatewayUrl, isUsableAddress } from "./addresses.ts";
import { Backoff } from "./backoff.ts";
import { utf8Encode, wipe } from "./bytes.ts";
import {
  type ClientEvents,
  type ClientOptions,
  type ClientTiming,
  type ConnectAttempt,
  type ConnectionState,
  type DaemonEvent,
  DEFAULT_CLIENT_TIMING,
  type ForgetReason,
  type GatewayInfo,
  type RawRequestOptions,
  type RequestOptions,
} from "./client-types.ts";
import { ControlQueue } from "./control-queue.ts";
import { HandshakeCounter } from "./counter.ts";
import { Emitter } from "./emitter.ts";
import { OverseerError, RequestError } from "./errors.ts";
import { MAX_REQUEST_BYTES } from "./frames.ts";
import { isJsonObject, type JsonObject, type JsonValue, numberField, stringField } from "./json.ts";
import { fingerprint, generateKeyPair, type HandshakeKind } from "./noise.ts";
import type { OutboxEntry } from "./outbox.ts";
import { decodePairingCode } from "./pairing-code.ts";
import { type Pairing, PairingStore } from "./pairing-store.ts";
import { platformTimers, type TimerHandle, type Timers } from "./platform.ts";
import { PendingRequests } from "./requests.ts";
import { SavedValue } from "./saved-value.ts";
import { ConnectError, type ConnectFailureCode, connectSession, type DeviceHello, PROTOCOL_VERSION, type Session, type SessionAttempt } from "./session.ts";
import { isRequestId, uuidV4 } from "./uuid.ts";

/** The class of a method the protocol description knows, or undefined. */
function classOf(method: string): string | undefined {
  return Object.prototype.hasOwnProperty.call(METHOD_CLASS, method) ? (METHOD_CLASS as Readonly<Record<string, string>>)[method] : undefined;
}

/** Pairing failed at every address. `attempts` says how at each. */
export class PairingError extends OverseerError {
  readonly attempts: readonly ConnectAttempt[];

  constructor(attempts: readonly ConnectAttempt[]) {
    super("pairing_failed", "pairing did not succeed at any address of the code");
    this.name = "PairingError";
    this.attempts = attempts;
  }
}

export class PhoneClient {
  private readonly options: ClientOptions;
  private readonly timing: ClientTiming;
  private readonly timers: Timers;
  private readonly emitter: Emitter<ClientEvents>;
  private readonly pairings: PairingStore;
  private readonly counter: HandshakeCounter;
  private readonly requests: PendingRequests;
  private readonly control: ControlQueue;
  private readonly savedCursor: SavedValue;
  private readonly savedContact: SavedValue;
  private readonly savedAddress: SavedValue;
  private readonly backoff: Backoff;

  private current: ConnectionState = "unpaired";
  private pairing: Pairing | null = null;
  private running = false;
  private loaded = false;
  private generation = 0;
  private session: Session | null = null;
  private sessionDone: Promise<void> | null = null;
  private greeted = false;
  private liveNow = false;
  private attempt: SessionAttempt | null = null;
  private endSleep: (() => void) | null = null;
  private hurry = false;
  private pairingNow = false;
  private cursorValue = 0;
  private contactAt: number | null = null;
  private contactSavedAt: number | null = null;
  private lastAddress: Address | null = null;
  private discovered: readonly AddressInput[] = [];
  private extras: readonly AddressInput[];
  private helloResult: JsonObject | null = null;

  constructor(options: ClientOptions) {
    this.options = options;
    this.timing = { ...DEFAULT_CLIENT_TIMING, ...options.timing };
    this.timers = options.timers ?? platformTimers;
    this.emitter = new Emitter<ClientEvents>(options.log);
    const namespace = options.namespace ?? "overseer.";
    this.pairings = new PairingStore(options.store, options.secrets, namespace, options.log);
    this.counter = new HandshakeCounter(options.store, `${namespace}counter`, options.now);
    this.requests = new PendingRequests(this.timers);
    this.control = new ControlQueue(options.store, `${namespace}outbox`, {
      now: options.now,
      timers: this.timers,
      log: options.log,
      retryWindowMs: this.timing.retryWindowMs,
      session: () => (this.session && this.greeted ? this.session : null),
      send: (session, entry) => this.requests.call(session, entry.method, entry.params, 0, entry.requestId),
      changed: (entry) => this.emitter.emit("outbox", entry),
    });
    this.savedCursor = new SavedValue(options.store, `${namespace}cursor`, options.log);
    this.savedContact = new SavedValue(options.store, `${namespace}lastContact`, options.log);
    this.savedAddress = new SavedValue(options.store, `${namespace}lastAddress`, options.log);
    this.backoff = new Backoff(this.timing.backoffMinMs, this.timing.backoffMaxMs, options.random);
    this.extras = options.extras ?? [];
  }

  // ------------------------------------------------------------------ what the app reads

  /** The state of the connection. */
  get state(): ConnectionState {
    return this.current;
  }

  /** When the last frame arrived from the gateway, or null when none ever did. */
  get lastContact(): number | null {
    return this.contactAt;
  }

  /**
   * True when events are news: the session is up and the daemon finished replaying what was
   * missed. Until then events are history.
   */
  get live(): boolean {
    return this.liveNow;
  }

  /** The sequence number of the last event the app received. */
  get cursor(): number {
    return this.cursorValue;
  }

  /** The paired gateway and this device, or null when the app is not paired. */
  get gateway(): GatewayInfo | null {
    const p = this.pairing;
    if (!p) return null;
    return {
      deviceId: p.deviceId,
      deviceName: p.deviceName,
      platform: p.platform,
      gatewayName: p.gatewayName,
      gatewayFingerprint: fingerprint(p.gatewayPublicKey),
      scope: p.scope,
      pairedAt: p.pairedAt,
    };
  }

  /** The result of the last `hello`, or null before the first. */
  get hello(): JsonObject | null {
    return this.helloResult;
  }

  /** The outbox: what waits or is on the wire, in order, then what was answered lately. */
  outbox(): readonly OutboxEntry[] {
    return this.control.entries();
  }

  /** Removes an answered entry from the outbox, when the app has shown it. */
  dismiss(requestId: string): void {
    this.control.dismiss(requestId);
  }

  /** Adds a listener. The returned function removes it. */
  on<K extends keyof ClientEvents>(event: K, listener: ClientEvents[K]): () => void {
    return this.emitter.on(event, listener);
  }

  // ------------------------------------------------------------------ start, stop, wake

  /** Reads what is stored and, when paired, connects and keeps connected. */
  async start(): Promise<void> {
    if (this.running) return;
    this.running = true;
    const generation = ++this.generation;
    await this.load();
    if (generation !== this.generation) return;
    if (!this.pairing) {
      this.setState("unpaired");
      return;
    }
    this.setState("connecting");
    void this.run(generation);
  }

  /**
   * Ends the session and the retries, and stores what is not stored yet. The state afterwards
   * is `reconnecting` when paired: nothing is live until `start` is called again.
   */
  async stop(): Promise<void> {
    this.running = false;
    this.generation += 1;
    this.attempt?.cancel();
    this.session?.close();
    this.endSleep?.();
    if (this.sessionDone) await this.sessionDone;
    this.saveContact(true);
    await Promise.all([this.savedCursor.flush(), this.savedContact.flush(), this.savedAddress.flush(), this.control.settled()]);
    if (this.pairing && this.current !== "revoked") this.setState("reconnecting");
  }

  /**
   * The app came to the foreground or the network changed: retry at once. On a live session
   * the gateway is asked for a sign of life, and the session is replaced when none comes.
   */
  wake(): void {
    if (!this.running || !this.pairing) return;
    this.backoff.reset();
    if (this.session && this.greeted) {
      this.hurry = true;
      this.session.probe(this.timing.probeTimeoutMs);
    } else if (this.endSleep) {
      this.endSleep();
    } else {
      this.hurry = true;
    }
  }

  /** Addresses found on the network for the gateway's fingerprint. May be called at any time. */
  setDiscovered(addresses: readonly AddressInput[]): void {
    const known = new Set(this.candidates().map(addressKey));
    this.discovered = addresses.filter(isUsableAddress);
    const found = this.candidates().some((address) => !known.has(addressKey(address)));
    if (found && !this.session) this.wake();
  }

  /** Addresses the platform adds, tried last. May be called at any time. */
  setExtras(addresses: readonly AddressInput[]): void {
    this.extras = addresses.filter(isUsableAddress);
  }

  // ------------------------------------------------------------------ pairing

  /**
   * Pairs with the gateway of `code`: creates this device's keys, proves knowledge of the
   * pairing secret, waits while the owner confirms on the Mac, and stores the result. It
   * happens once; afterwards the client connects by itself, now and after every restart.
   */
  async pair(code: string, deviceName: string, platform: string): Promise<GatewayInfo> {
    if (this.pairingNow) throw new OverseerError("already_paired", "pairing is in progress");
    this.pairingNow = true;
    try {
      if (!this.loaded) await this.load();
      if (this.pairing) throw new OverseerError("already_paired", "this app is paired already; pairing happens once");
      const decoded = decodePairingCode(code);
      const keys = generateKeyPair(this.options.random);
      const attempts: ConnectAttempt[] = [];
      // The addresses of the code, then the platform's extras. Nothing else is asked for a pairing.
      const addresses = candidateAddresses({ last: null, discovered: [], paired: decoded.addresses, extras: this.extras, port: decoded.port });
      try {
        for (const address of addresses) {
          const session = await this.tryAddress("pairing", address, {
            staticPrivateKey: keys.privateKey,
            staticPublicKey: keys.publicKey,
            gatewayPublicKey: decoded.gatewayPublicKey,
            pairingSecret: decoded.secret,
            hello: { device: "", name: deviceName, platform, app: this.options.app },
          }).catch((error: unknown) => {
            attempts.push(this.report("pairing", address, error instanceof ConnectError ? error.failure : "unreachable"));
            return null;
          });
          if (!session) continue;
          // The session of a pairing is not used: every session starts the same way, by the keys.
          session.close();
          attempts.push(this.report("pairing", address, "connected"));
          const pairing: Pairing = {
            deviceId: session.gateway.device,
            deviceName,
            platform,
            gatewayName: session.gateway.gateway,
            scope: session.gateway.scope,
            gatewayPublicKey: decoded.gatewayPublicKey,
            devicePrivateKey: keys.privateKey,
            devicePublicKey: keys.publicKey,
            port: decoded.port,
            addresses: decoded.addresses,
            pairedAt: this.options.now(),
          };
          await this.pairings.save(pairing);
          this.pairing = pairing;
          this.cursorValue = 0;
          this.savedCursor.set("0");
          this.rememberAddress(address);
          const info = this.gateway as GatewayInfo;
          this.emitter.emit("paired", info);
          this.running = true;
          this.setState("connecting");
          void this.run(++this.generation);
          return info;
        }
      } finally {
        wipe(decoded.secret);
      }
      wipe(keys.privateKey);
      throw new PairingError(attempts);
    } finally {
      this.pairingNow = false;
    }
  }

  /** Removes the pairing from this phone. The Mac still lists the device until it is revoked there. */
  async forget(): Promise<void> {
    await this.erase("forgotten", "unpaired");
  }

  // ------------------------------------------------------------------ requests

  /**
   * Sends a request of the protocol and resolves with its result, or rejects with a
   * `RequestError` when the reply is an error.
   *
   * The method's class decides how it is sent. A method of class `control` changes something:
   * it carries a request id, is written to the outbox, is sent when there is a session, and is
   * sent again with the same request id after every reconnect and restart until it is
   * answered. Any other method needs a session now.
   */
  request<M extends PhoneMethod>(method: M, params: Params<M>, options: RequestOptions = {}): Promise<Result<M>> {
    return this.requestRaw(method, params, options) as Promise<Result<M>>;
  }

  /**
   * The same by name, without the protocol's types: for tests and for methods that were added
   * to the daemon after this app was built. `params` must be a JSON object.
   */
  requestRaw(method: string, params: unknown = {}, options: RawRequestOptions = {}): Promise<JsonValue> {
    const object = params === undefined || params === null ? {} : params;
    if (!isJsonObject(object)) return Promise.reject(new OverseerError("invalid_params", "the parameters of a request are an object"));
    try {
      JSON.stringify(object);
    } catch {
      return Promise.reject(new OverseerError("invalid_params", "the parameters of a request must be JSON"));
    }
    const known = classOf(method);
    const control = known === undefined ? options.control === true : known === "control";
    if (control) return this.requestControl(method, object, options);
    const session = this.session;
    if (!this.pairing) return Promise.reject(new OverseerError("unpaired", "this app is not paired"));
    if (!session || !this.greeted) return Promise.reject(new OverseerError("not_connected", "there is no connection to the Mac"));
    return this.requests.call(session, method, object, options.timeoutMs ?? this.timing.requestTimeoutMs);
  }

  private requestControl(method: string, params: JsonObject, options: RequestOptions): Promise<JsonValue> {
    if (!this.pairing) return Promise.reject(new OverseerError("unpaired", "this app is not paired"));
    const requestId = options.requestId ?? uuidV4(this.options.random);
    if (!isRequestId(requestId)) return Promise.reject(new OverseerError("bad_request_id", "a request id is 8 to 64 letters, digits and hyphens"));
    const size = utf8Encode(JSON.stringify({ id: Number.MAX_SAFE_INTEGER, method, params, request_id: requestId })).length;
    if (size > MAX_REQUEST_BYTES) return Promise.reject(new OverseerError("request_too_large", "a request may be 1 MiB at most"));
    return this.control.submit(requestId, method, params, options.timeoutMs);
  }

  // ------------------------------------------------------------------ connecting

  private candidates(): Address[] {
    return candidateAddresses({
      last: this.lastAddress,
      discovered: this.discovered,
      paired: this.pairing?.addresses ?? [],
      extras: this.extras,
      port: this.pairing?.port ?? 0,
    });
  }

  private tryAddress(
    kind: HandshakeKind,
    address: Address,
    keys: { staticPrivateKey: Uint8Array; staticPublicKey: Uint8Array; gatewayPublicKey: Uint8Array; pairingSecret?: Uint8Array; hello: DeviceHello },
  ): Promise<Session> {
    const attempt = connectSession({
      url: gatewayUrl(address),
      kind,
      socketFactory: this.options.socketFactory,
      counter: this.counter,
      random: this.options.random,
      now: this.options.now,
      timers: this.timers,
      timing: this.timing,
      onActivity: (at) => this.heard(at),
      ...(this.options.log ? { log: this.options.log } : {}),
      ...keys,
    });
    this.attempt = attempt;
    return attempt.result.finally(() => {
      if (this.attempt === attempt) this.attempt = null;
    });
  }

  private report(kind: HandshakeKind, address: Address, outcome: ConnectAttempt["outcome"]): ConnectAttempt {
    const attempt: ConnectAttempt = { address, url: gatewayUrl(address), kind, outcome, at: this.options.now() };
    this.options.log?.(`${kind} at ${formatAddress(address)}: ${outcome}`);
    this.emitter.emit("attempt", attempt);
    return attempt;
  }

  /** Connects and keeps connected until the generation changes. */
  private async run(generation: number): Promise<void> {
    while (this.generation === generation) {
      const connected = await this.connectPass(generation);
      if (this.generation !== generation) return;
      if (connected) {
        // The session is up. Nothing more is tried until it has ended and everything it
        // delivered was applied, so two sessions never feed the app at once.
        if (this.sessionDone) await this.sessionDone;
        if (this.generation !== generation) return;
      } else if (this.current !== "off") {
        this.setState("unreachable");
      }
      await this.sleep(this.current === "off" ? this.timing.offRetryMs : this.backoff.next());
    }
  }

  /**
   * Tries every address once, in order. True when one of them became the session.
   * (It returns a plain value on purpose: an async function that returns a promise waits for it.)
   */
  private async connectPass(generation: number): Promise<boolean> {
    for (const address of this.candidates()) {
      const pairing = this.pairing;
      if (this.generation !== generation || !pairing) return false;
      let session: Session;
      try {
        session = await this.tryAddress("session", address, {
          staticPrivateKey: pairing.devicePrivateKey,
          staticPublicKey: pairing.devicePublicKey,
          gatewayPublicKey: pairing.gatewayPublicKey,
          hello: { device: pairing.deviceId, name: pairing.deviceName, platform: pairing.platform, app: this.options.app },
        });
      } catch (error) {
        if (this.generation !== generation) return false;
        const failure: ConnectFailureCode = error instanceof ConnectError ? error.failure : "unreachable";
        this.report("session", address, failure);
        continue;
      }
      if (this.generation !== generation) {
        session.close();
        return false;
      }
      const ended = this.adopt(session);
      if (await this.greet(session, generation)) {
        this.rememberAddress(address);
        this.report("session", address, "connected");
        this.setState("online");
        void this.subscribe(session);
        void this.control.pump();
        return true;
      }
      session.close();
      await ended;
      if (this.generation !== generation) return false;
      this.report("session", address, "hello_failed");
    }
    return false;
  }

  private adopt(session: Session): Promise<void> {
    this.session = session;
    this.greeted = false;
    this.liveNow = false;
    const ended = new Promise<void>((resolve) => {
      session.listen({
        onMessage: (value) => this.receive(session, value),
        onClose: () => {
          this.sessionClosed(session);
          resolve();
        },
      });
    });
    this.sessionDone = ended;
    return ended;
  }

  /** Says `hello` as client `phone`. True when the gateway answered and speaks this protocol. */
  private async greet(session: Session, generation: number): Promise<boolean> {
    let result: JsonValue;
    try {
      result = await this.requests.call(session, "hello", { client: "phone" }, this.timing.handshakeTimeoutMs);
    } catch {
      return false;
    }
    if (this.generation !== generation || session.closed || this.session !== session) return false;
    if (isJsonObject(result)) {
      const protocol = numberField(result, "protocol");
      if (protocol !== null && protocol !== PROTOCOL_VERSION) return false;
      this.helloResult = result;
    }
    this.greeted = true;
    this.hurry = false;
    const pairing = this.pairing;
    if (pairing && (pairing.gatewayName !== session.gateway.gateway || pairing.scope !== session.gateway.scope)) {
      this.pairing = { ...pairing, gatewayName: session.gateway.gateway, scope: session.gateway.scope };
      this.pairings.saveRecord(this.pairing).catch(() => this.options.log?.("the pairing could not be stored"));
    }
    return true;
  }

  /** Subscribes to events after the cursor. */
  private async subscribe(session: Session): Promise<void> {
    const after = this.cursorValue;
    try {
      const result = await this.requests.call(session, "events.subscribe", { after }, this.timing.requestTimeoutMs);
      // Only now did connecting succeed in full, and the waits between passes start over. A
      // gateway that greets and then refuses the subscription is retried ever more slowly.
      this.backoff.reset();
      if (isJsonObject(result) && result["history_truncated"] === true) {
        // The daemon's log may have started again below the cursor (another data folder, a
        // restored backup). It then goes on from the end of its log and says where that is:
        // the cursor follows, or every event up to the old cursor would be dropped as seen.
        const from = numberField(result, "after");
        if (from !== null && from < this.cursorValue) {
          this.cursorValue = from;
          this.savedCursor.set(String(from));
        }
        this.emitter.emit("truncated", { cursor: this.cursorValue });
      }
    } catch (error) {
      if (session.closed) return;
      this.options.log?.(`subscribing failed (${error instanceof RequestError ? error.code : "no reply"}); connecting again`);
      session.close();
    }
  }

  private rememberAddress(address: Address): void {
    if (this.lastAddress && addressKey(this.lastAddress) === addressKey(address)) return;
    this.lastAddress = address;
    this.savedAddress.set(JSON.stringify({ host: address.host, port: address.port }));
  }

  private sleep(ms: number): Promise<void> {
    if (this.hurry) {
      this.hurry = false;
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => {
      let timer: TimerHandle | null = null;
      const done = (): void => {
        if (timer !== null) this.timers.clear(timer);
        timer = null;
        if (this.endSleep === done) this.endSleep = null;
        resolve();
      };
      this.endSleep = done;
      timer = this.timers.set(done, ms);
    });
  }

  // ------------------------------------------------------------------ what arrives

  private heard(at: number): void {
    this.contactAt = at;
    this.saveContact(false);
  }

  private saveContact(now: boolean): void {
    if (this.contactAt === null || this.contactAt === this.contactSavedAt) return;
    if (!now && this.contactSavedAt !== null && Math.abs(this.contactAt - this.contactSavedAt) < this.timing.contactSaveMs) return;
    this.contactSavedAt = this.contactAt;
    this.savedContact.set(String(this.contactAt));
  }

  /** One message of the session. They arrive one at a time, in order. */
  private async receive(session: Session, value: JsonValue): Promise<void> {
    if (!isJsonObject(value)) return;
    const method = value["method"];
    if (typeof method === "string") {
      const params = value["params"] ?? null;
      if (method === "event") await this.applyEvent(params);
      else if (method === "replayed") {
        this.liveNow = true;
        this.emitter.emit("replayed", isJsonObject(params) ? (numberField(params, "cursor") ?? this.cursorValue) : this.cursorValue);
      } else if (method === "resync") {
        // The daemon's stream lagged behind: what was missed is replayed from the cursor.
        this.liveNow = false;
        void this.subscribe(session);
      }
      else if (method === "gateway") this.notice(session, isJsonObject(params) ? stringField(params, "state") : null);
      else this.emitter.emit("notification", method, params);
      return;
    }
    const error = this.requests.answer(session, value);
    // The gateway says so in a reply as well as in a notice: this device was revoked.
    if (error?.code === "revoked") void this.erase("revoked", "revoked");
  }

  private async applyEvent(params: JsonValue): Promise<void> {
    if (!isJsonObject(params)) return;
    const seq = params["seq"];
    if (typeof seq !== "number" || !Number.isSafeInteger(seq)) return;
    // Replay overlap: what was applied is never applied twice.
    if (seq <= this.cursorValue) return;
    // The daemon is authenticated and its protocol version was checked: its events have the
    // shape the protocol describes. Only `seq`, which the cursor depends on, is checked here.
    await this.emitter.emitAndWait("event", params as unknown as DaemonEvent, { live: this.liveNow });
    if (!this.pairing) return;
    this.cursorValue = seq;
    this.savedCursor.set(String(seq));
  }

  private notice(session: Session, state: string | null): void {
    if (state === "off") {
      this.setState("off");
      session.close();
    } else if (state === "revoked") {
      void this.erase("revoked", "revoked");
    }
  }

  private sessionClosed(session: Session): void {
    if (this.session === session) {
      this.session = null;
      this.greeted = false;
      this.liveNow = false;
    }
    this.requests.failAll(new OverseerError("connection_lost", "the connection to the Mac was lost before the reply"), session);
    this.control.sessionEnded();
    this.saveContact(true);
    if (this.current === "online") this.setState("reconnecting");
  }

  // ------------------------------------------------------------------ state and storage

  private setState(next: ConnectionState): void {
    const previous = this.current;
    if (previous === next) return;
    this.current = next;
    this.options.log?.(`state: ${previous} -> ${next}`);
    this.emitter.emit("state", next, previous);
  }

  private async load(): Promise<void> {
    const [pairing, cursor, contact, address] = await Promise.all([this.pairings.load(), this.savedCursor.load(), this.savedContact.load(), this.savedAddress.load()]);
    await this.control.load();
    this.pairing = pairing;
    const seq = cursor === null ? 0 : Number(cursor);
    this.cursorValue = Number.isSafeInteger(seq) && seq > 0 ? seq : 0;
    const at = contact === null ? Number.NaN : Number(contact);
    this.contactAt = Number.isFinite(at) ? at : null;
    this.contactSavedAt = this.contactAt;
    this.lastAddress = null;
    if (address !== null) {
      try {
        const parsed: unknown = JSON.parse(address);
        if (isJsonObject(parsed)) {
          const candidate = { host: stringField(parsed, "host") ?? "", port: numberField(parsed, "port") ?? 0 };
          if (isUsableAddress(candidate)) this.lastAddress = candidate;
        }
      } catch {
        this.options.log?.("the last address cannot be read; it is not used");
      }
    }
    this.loaded = true;
  }

  /** Forgets the pairing: ends everything, deletes the keys and what belongs to the pairing. */
  private async erase(reason: ForgetReason, state: "revoked" | "unpaired"): Promise<void> {
    if (!this.pairing) return;
    const generation = ++this.generation;
    this.pairing = null;
    this.running = false;
    this.setState(state);
    this.requests.failAll(new OverseerError("unpaired", reason === "revoked" ? "this phone was revoked on the Mac" : "this phone is no longer paired"));
    this.attempt?.cancel();
    this.session?.close();
    this.endSleep?.();
    const emptied = this.control.failAll({ code: reason === "revoked" ? "revoked" : "unpaired", message: "this phone is no longer paired" });
    this.cursorValue = 0;
    this.contactAt = null;
    this.contactSavedAt = null;
    this.lastAddress = null;
    this.helloResult = null;
    this.savedCursor.set(null);
    this.savedContact.set(null);
    this.savedAddress.set(null);
    try {
      await this.pairings.forget();
      await Promise.all([emptied, this.savedCursor.flush(), this.savedContact.flush(), this.savedAddress.flush()]);
    } catch {
      this.options.log?.("the pairing could not be deleted from storage");
    }
    try {
      await this.options.onForget?.(reason);
    } catch {
      this.options.log?.("the app's cleanup after forgetting the pairing failed");
    }
    this.emitter.emit("forgotten", reason);
    if (this.generation === generation) this.setState("unpaired");
  }
}
