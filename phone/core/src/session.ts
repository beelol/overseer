/**
 * One encrypted session over a socket (docs/rfcs/phone-remote-protocol.md).
 *
 * `connectSession` opens the socket, runs the handshake (a session of a paired device, or the
 * pairing itself) and returns a `Session` that sends and receives JSON values. Any frame that
 * does not decrypt, breaks the framing or is not JSON ends the session.
 */

import { utf8Decode, utf8Encode } from "./bytes.ts";
import type { HandshakeCounter } from "./counter.ts";
import { type ConnectFailure, OverseerError } from "./errors.ts";
import { MAX_REPLY_BYTES, MAX_REQUEST_BYTES, Opener, seal } from "./frames.ts";
import { isJsonObject, type JsonObject, type JsonValue } from "./json.ts";
import { type CipherState, fingerprint, Handshake, type HandshakeKind, pskFromSecret } from "./noise.ts";
import { type Clock, type Log, platformTimers, type RandomSource, type TimerHandle, type Timers } from "./platform.ts";
import type { Socket, SocketFactory, SocketHandlers } from "./socket.ts";

export const FRAME_VERSION = 0x01;
export const KIND_SESSION = 0x01;
export const KIND_PAIRING = 0x02;
/** The gateway refuses a first frame longer than this. */
export const MAX_FIRST_FRAME = 4096;
/** The protocol version this library speaks. */
export const PROTOCOL_VERSION = 1;
/** The id of the keepalive request. Requests of the client start at 1. */
export const KEEPALIVE_ID = 0;

/** Waiting times of a session, in milliseconds. */
export interface SessionTiming {
  /** How long the socket may take to open. */
  readonly openTimeoutMs: number;
  /** How long the gateway may take to answer a session handshake. */
  readonly handshakeTimeoutMs: number;
  /** The same for pairing, where the owner confirms on the Mac (60 s at most). */
  readonly pairingTimeoutMs: number;
  /** A keepalive is sent this often. */
  readonly keepaliveMs: number;
  /** The session ends when nothing was received for this long. */
  readonly idleTimeoutMs: number;
}

export const DEFAULT_SESSION_TIMING: SessionTiming = {
  openTimeoutMs: 3_000,
  handshakeTimeoutMs: 10_000,
  pairingTimeoutMs: 70_000,
  keepaliveMs: 20_000,
  idleTimeoutMs: 60_000,
};

/** What the device says about itself in the first handshake message. */
export interface DeviceHello {
  /** The device id from pairing; empty while pairing. */
  readonly device: string;
  readonly name: string;
  /** `ios` or `android`. */
  readonly platform: string;
  /** The app's version. */
  readonly app: string;
}

/** What the gateway says about itself in the second handshake message. */
export interface GatewayHello {
  readonly protocol: number;
  /** This device's id. */
  readonly device: string;
  /** `full` or `watch`. */
  readonly scope: string;
  /** The Mac's name. */
  readonly gateway: string;
  /** The fingerprint the gateway states. The handshake, not this text, proves the key. */
  readonly fingerprint: string;
}

/** Why a session ended. */
export type SessionEndReason =
  /** The gateway or the network closed the connection. */
  | "closed_by_peer"
  /** This side closed it. */
  | "closed_locally"
  /** A frame did not decrypt, broke the framing or was not JSON. */
  | "protocol"
  /** Nothing was received for too long. */
  | "idle"
  /** Sending failed. */
  | "socket_error";

export interface SessionEnd {
  readonly reason: SessionEndReason;
  /** The error code behind a `protocol` end, for diagnostics. Never message content. */
  readonly detail: string;
}

/** Receives what a session delivers. */
export interface SessionHandlers {
  /**
   * One message of the daemon's protocol. Messages are delivered one at a time and in order:
   * the next one waits until the promise of this one resolved.
   */
  onMessage(value: JsonValue): void | Promise<void>;
  /** The session ended. Called once, after the last message. */
  onClose(end: SessionEnd): void;
}

/** How a connection attempt can fail: what the peer did, or what stopped this side. */
export type ConnectFailureCode =
  | ConnectFailure
  /** The handshake counter could not be stored, so nothing was sent. */
  | "storage"
  /** The first frame would be longer than the gateway accepts. */
  | "length"
  /** The gateway key or the device key is not usable. */
  | "bad_key";

/** A connection attempt failed. `failure` says how. */
export class ConnectError extends OverseerError {
  readonly failure: ConnectFailureCode;

  constructor(failure: ConnectFailureCode, message: string) {
    super(failure, message);
    this.name = "ConnectError";
    this.failure = failure;
  }
}

/** Everything one connection attempt needs. */
export interface SessionConfig {
  readonly url: string;
  readonly kind: HandshakeKind;
  readonly socketFactory: SocketFactory;
  /** The device's static private key. */
  readonly staticPrivateKey: Uint8Array;
  /** The device's static public key, when known; it saves a computation. */
  readonly staticPublicKey?: Uint8Array;
  /** The gateway's static public key, from the pairing code. */
  readonly gatewayPublicKey: Uint8Array;
  /** The pairing secret from the pairing code. Pairing only. */
  readonly pairingSecret?: Uint8Array;
  readonly hello: DeviceHello;
  readonly counter: HandshakeCounter;
  readonly random: RandomSource;
  readonly now: Clock;
  readonly timers?: Timers;
  readonly log?: Log;
  readonly timing?: Partial<SessionTiming>;
  /** The method of the keepalive request. The gateway has `ping`. */
  readonly keepaliveMethod?: string;
  /** Limit of a joined inbound message. */
  readonly maxInboundBytes?: number;
  /** Called for every frame received, with the time. */
  readonly onActivity?: (at: number) => void;
  /** FOR TESTS ONLY: a fixed ephemeral key. */
  readonly ephemeralPrivateKey?: Uint8Array;
}

/** A running connection attempt. */
export interface SessionAttempt {
  /** Resolves with the session, or rejects with a `ConnectError`. */
  readonly result: Promise<Session>;
  /** Gives the attempt up. `result` rejects with `cancelled` unless it was settled already. */
  cancel(): void;
}

/** An established session. */
export interface Session {
  readonly url: string;
  readonly kind: HandshakeKind;
  readonly gateway: GatewayHello;
  /** The handshake hash: unique to this session, equal on both sides. */
  readonly handshakeHash: Uint8Array;
  /** The key the gateway proved it holds. */
  readonly gatewayPublicKey: Uint8Array;
  readonly closed: boolean;
  /** Starts delivery. Messages received before this call were kept and are delivered first. */
  listen(handlers: SessionHandlers): void;
  /** Sends one protocol message. Throws when the session is closed or the message is over 1 MiB. */
  send(value: JsonValue): void;
  /** Asks the gateway for a sign of life now; ends the session when none comes in time. */
  probe(timeoutMs: number): void;
  close(): void;
}

class LiveSession implements Session {
  readonly url: string;
  readonly kind: HandshakeKind;
  readonly gateway: GatewayHello;
  readonly handshakeHash: Uint8Array;
  readonly gatewayPublicKey: Uint8Array;

  private readonly socket: Socket;
  private readonly sendCipher: CipherState;
  private readonly receiveCipher: CipherState;
  private readonly opener: Opener;
  private readonly timers: Timers;
  private readonly now: Clock;
  private readonly log: Log | undefined;
  private readonly timing: SessionTiming;
  private readonly keepaliveMethod: string;
  private readonly onActivity: ((at: number) => void) | undefined;

  private handlers: SessionHandlers | null = null;
  private readonly inbox: JsonValue[] = [];
  private delivering = false;
  private end: SessionEnd | null = null;
  private endDelivered = false;
  private keepaliveTimer: TimerHandle | null = null;
  private probeTimer: TimerHandle | null = null;
  private heard = false;
  private quietMs = 0;

  constructor(init: {
    config: SessionConfig;
    socket: Socket;
    send: CipherState;
    receive: CipherState;
    gateway: GatewayHello;
    handshakeHash: Uint8Array;
    gatewayPublicKey: Uint8Array;
    timing: SessionTiming;
  }) {
    const { config } = init;
    this.url = config.url;
    this.kind = config.kind;
    this.gateway = init.gateway;
    this.handshakeHash = init.handshakeHash;
    this.gatewayPublicKey = init.gatewayPublicKey;
    this.socket = init.socket;
    this.sendCipher = init.send;
    this.receiveCipher = init.receive;
    this.opener = new Opener(config.maxInboundBytes ?? MAX_REPLY_BYTES);
    this.timers = config.timers ?? platformTimers;
    this.now = config.now;
    this.log = config.log;
    this.timing = init.timing;
    this.keepaliveMethod = config.keepaliveMethod ?? "ping";
    this.onActivity = config.onActivity;
    this.scheduleKeepalive();
  }

  get closed(): boolean {
    return this.end !== null;
  }

  listen(handlers: SessionHandlers): void {
    if (this.handlers) throw new OverseerError("closed", "this session has a listener already");
    this.handlers = handlers;
    void this.deliver();
  }

  send(value: JsonValue): void {
    if (this.end) throw new OverseerError("closed", "the session is closed");
    const bytes = utf8Encode(JSON.stringify(value));
    if (bytes.length > MAX_REQUEST_BYTES) throw new OverseerError("request_too_large", "a request may be 1 MiB at most");
    let frames: Uint8Array[];
    try {
      frames = seal(this.sendCipher, bytes);
    } catch (error) {
      this.finish("protocol", error instanceof OverseerError ? error.code : "seal");
      throw new OverseerError("closed", "the session is closed");
    }
    try {
      for (const frame of frames) this.socket.send(frame);
    } catch {
      this.finish("socket_error", "send");
      throw new OverseerError("closed", "the session is closed");
    }
  }

  probe(timeoutMs: number): void {
    if (this.end || this.probeTimer !== null) return;
    this.heard = false;
    this.probeTimer = this.timers.set(() => {
      this.probeTimer = null;
      if (!this.heard) this.finish("idle", "no answer to a probe");
    }, timeoutMs);
    this.sendKeepalive();
  }

  close(): void {
    this.finish("closed_locally", "");
  }

  /** One frame from the socket. */
  receive(frame: Uint8Array): void {
    if (this.end) return;
    let value: JsonValue;
    try {
      const message = this.opener.open(this.receiveCipher, frame);
      // Only a frame that proved to be the gateway's counts as a sign of life.
      this.heard = true;
      this.quietMs = 0;
      this.onActivity?.(this.now());
      if (message === null) return;
      value = JSON.parse(utf8Decode(message)) as JsonValue;
    } catch (error) {
      this.finish("protocol", error instanceof OverseerError ? error.code : "not JSON");
      return;
    }
    if (isJsonObject(value) && value["id"] === KEEPALIVE_ID && value["method"] === undefined) return;
    this.inbox.push(value);
    void this.deliver();
  }

  /** The socket closed. */
  socketClosed(): void {
    this.finish("closed_by_peer", "");
  }

  private scheduleKeepalive(): void {
    this.keepaliveTimer = this.timers.set(() => {
      this.keepaliveTimer = null;
      if (this.end) return;
      this.quietMs = this.heard ? 0 : this.quietMs + this.timing.keepaliveMs;
      this.heard = false;
      if (this.quietMs >= this.timing.idleTimeoutMs) {
        this.finish("idle", "nothing received");
        return;
      }
      this.sendKeepalive();
      if (!this.end) this.scheduleKeepalive();
    }, this.timing.keepaliveMs);
  }

  private sendKeepalive(): void {
    this.socket.ping?.();
    try {
      this.send({ id: KEEPALIVE_ID, method: this.keepaliveMethod, params: {} });
    } catch {
      // `send` ended the session already.
    }
  }

  private finish(reason: SessionEndReason, detail: string): void {
    if (this.end) return;
    this.end = { reason, detail };
    if (this.keepaliveTimer !== null) this.timers.clear(this.keepaliveTimer);
    if (this.probeTimer !== null) this.timers.clear(this.probeTimer);
    this.keepaliveTimer = null;
    this.probeTimer = null;
    this.sendCipher.destroy();
    this.receiveCipher.destroy();
    // What this side gave up is not delivered; it was not applied, so the cursor did not move.
    if (reason === "closed_locally") this.inbox.length = 0;
    if (reason !== "closed_by_peer") this.socket.close();
    if (reason !== "closed_locally" && reason !== "closed_by_peer") this.log?.(`session ended: ${reason} (${detail})`);
    void this.deliver();
  }

  private async deliver(): Promise<void> {
    if (this.delivering || !this.handlers) return;
    this.delivering = true;
    const handlers = this.handlers;
    try {
      while (this.inbox.length > 0) {
        const value = this.inbox.shift() as JsonValue;
        try {
          await handlers.onMessage(value);
        } catch {
          this.log?.("a message handler threw; the session continues");
        }
      }
    } finally {
      this.delivering = false;
    }
    if (this.end && !this.endDelivered && this.inbox.length === 0) {
      this.endDelivered = true;
      try {
        handlers.onClose(this.end);
      } catch {
        this.log?.("a close handler threw");
      }
    }
  }
}

function parseGatewayHello(payload: Uint8Array): GatewayHello | null {
  let value: unknown;
  try {
    value = JSON.parse(utf8Decode(payload));
  } catch {
    return null;
  }
  if (!isJsonObject(value)) return null;
  const hello = value as JsonObject;
  const { protocol, device, scope, gateway, fingerprint: print } = hello;
  // Without a device id there is nothing to pair with or to connect as.
  if (typeof protocol !== "number" || typeof device !== "string" || device.length === 0 || typeof scope !== "string") return null;
  return {
    protocol,
    device,
    scope,
    gateway: typeof gateway === "string" ? gateway : "",
    fingerprint: typeof print === "string" ? print : "",
  };
}

/**
 * Opens a socket to `config.url` and runs the handshake.
 *
 * The first frame is `[0x01][kind][Noise message 1]`. Its payload carries the counter, which is
 * in the store before the frame is sent. The gateway's answer is Noise message 2; the gateway
 * is whoever proves the key from pairing, whatever its address or name.
 */
export function connectSession(config: SessionConfig): SessionAttempt {
  const timers = config.timers ?? platformTimers;
  const timing: SessionTiming = { ...DEFAULT_SESSION_TIMING, ...config.timing };
  const answerTimeoutMs = config.kind === "pairing" ? timing.pairingTimeoutMs : timing.handshakeTimeoutMs;

  let settled = false;
  let socket: Socket | null = null;
  let handshake: Handshake | null = null;
  let session: LiveSession | null = null;
  let timer: TimerHandle | null = null;
  let opened = false;
  let sent = false;
  let resolveResult: (session: Session) => void = () => undefined;
  let rejectResult: (error: ConnectError) => void = () => undefined;
  const result = new Promise<Session>((resolve, reject) => {
    resolveResult = resolve;
    rejectResult = reject;
  });

  const stopTimer = (): void => {
    if (timer !== null) timers.clear(timer);
    timer = null;
  };
  const fail = (failure: ConnectFailureCode, message: string): void => {
    if (settled) return;
    settled = true;
    stopTimer();
    handshake?.destroy();
    socket?.close();
    rejectResult(new ConnectError(failure, message));
  };

  const start = async (): Promise<void> => {
    let counter: number;
    try {
      counter = await config.counter.next();
    } catch {
      fail("storage", "the handshake counter could not be stored");
      return;
    }
    if (settled) return;

    let firstFrame: Uint8Array;
    try {
      handshake = new Handshake({
        kind: config.kind,
        role: "initiator",
        staticPrivateKey: config.staticPrivateKey,
        remoteStaticPublicKey: config.gatewayPublicKey,
        random: config.random,
        ...(config.staticPublicKey ? { staticPublicKey: config.staticPublicKey } : {}),
        ...(config.kind === "pairing" && config.pairingSecret ? { psk: pskFromSecret(config.pairingSecret) } : {}),
        ...(config.ephemeralPrivateKey ? { ephemeralPrivateKey: config.ephemeralPrivateKey } : {}),
      });
      const payload = utf8Encode(
        JSON.stringify({
          device: config.hello.device,
          name: config.hello.name,
          platform: config.hello.platform,
          app: config.hello.app,
          counter,
        }),
      );
      const message1 = handshake.writeMessage(payload);
      firstFrame = new Uint8Array(2 + message1.length);
      firstFrame[0] = FRAME_VERSION;
      firstFrame[1] = config.kind === "pairing" ? KIND_PAIRING : KIND_SESSION;
      firstFrame.set(message1, 2);
    } catch {
      fail("bad_key", "the handshake could not be started with these keys");
      return;
    }
    if (firstFrame.length > MAX_FIRST_FRAME) {
      fail("length", "the device name is too long for the handshake");
      return;
    }

    timer = timers.set(() => fail(opened ? "timeout" : "unreachable", opened ? "the gateway did not answer in time" : "nothing answered at this address in time"), timing.openTimeoutMs);

    const handlers: SocketHandlers = {
      onOpen: () => {
        if (settled || opened) return;
        opened = true;
        stopTimer();
        timer = timers.set(() => fail("timeout", "the gateway did not answer the handshake in time"), answerTimeoutMs);
        try {
          made.send(firstFrame);
          sent = true;
        } catch {
          fail("unreachable", "the handshake could not be sent");
        }
      },
      onMessage: (frame) => {
        if (session) {
          session.receive(frame);
          return;
        }
        if (settled || !handshake || !sent) return;
        let hello: GatewayHello | null;
        try {
          hello = parseGatewayHello(handshake.readMessage(frame));
        } catch {
          fail("impostor", "the answer does not prove the gateway's key");
          return;
        }
        // Only now is it known that the gateway itself answered.
        config.onActivity?.(config.now());
        if (!hello || hello.protocol !== PROTOCOL_VERSION) {
          fail("incompatible", "the gateway speaks a protocol version this app does not");
          return;
        }
        const keys = handshake.split();
        handshake = null;
        if (hello.fingerprint !== fingerprint(keys.remoteStaticPublicKey)) {
          config.log?.("the gateway states a fingerprint that is not the one of its key; its key is what counts");
        }
        settled = true;
        stopTimer();
        session = new LiveSession({
          config,
          socket: made,
          send: keys.send,
          receive: keys.receive,
          gateway: hello,
          handshakeHash: keys.handshakeHash,
          gatewayPublicKey: keys.remoteStaticPublicKey,
          timing,
        });
        resolveResult(session);
      },
      onError: () => {
        // The close that follows decides what happened.
      },
      onClose: () => {
        if (session) {
          session.socketClosed();
          return;
        }
        if (!opened) fail("unreachable", "nothing answered at this address");
        else fail("refused", "the peer closed the connection without answering the handshake");
      },
    };
    let made: Socket;
    try {
      made = config.socketFactory(config.url, handlers);
    } catch {
      fail("unreachable", "no socket could be opened to this address");
      return;
    }
    socket = made;
    if (settled) made.close();
  };

  void start();

  return {
    result,
    cancel() {
      if (session) {
        session.close();
        return;
      }
      fail("cancelled", "the attempt was cancelled");
    },
  };
}
