import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { WebSocket as WsWebSocket } from "ws";
import { utf8Encode } from "../src/bytes.ts";
import { HandshakeCounter } from "../src/counter.ts";
import type { JsonObject, JsonValue } from "../src/json.ts";
import { generateKeyPair, type KeyPair } from "../src/noise.ts";
import { decodePairingCode } from "../src/pairing-code.ts";
import { MemoryStore } from "../src/platform.ts";
import { ConnectError, connectSession, KIND_PAIRING, KIND_SESSION, type Session, type SessionConfig, type SessionEnd } from "../src/session.ts";
import { realRandom, RecordingSockets, RecordingStore, sleep, waitFor, wsSockets } from "./helpers.ts";
import { MockGateway, type PairingRequest } from "./mock-gateway.ts";

let gateway: MockGateway;
let sockets: RecordingSockets;
let store: RecordingStore;
let clock: { now: number };
const open: Session[] = [];

beforeEach(async () => {
  gateway = new MockGateway();
  await gateway.start(0);
  sockets = new RecordingSockets();
  store = new RecordingStore();
  clock = { now: 1_790_000_000_000 };
});

afterEach(async () => {
  for (const session of open.splice(0)) session.close();
  await gateway.stop();
});

function url(port = gateway.port): string {
  return `ws://127.0.0.1:${port}/v1`;
}

function config(kind: "session" | "pairing", keys: KeyPair, extra: Partial<SessionConfig> = {}): SessionConfig {
  return {
    url: url(),
    kind,
    socketFactory: sockets.factory,
    staticPrivateKey: keys.privateKey,
    gatewayPublicKey: gateway.keys.publicKey,
    hello: { device: "", name: "Test Phone", platform: "ios", app: "0.1.0" },
    counter: new HandshakeCounter(store, "counter", () => clock.now),
    random: realRandom,
    now: () => clock.now,
    timing: { openTimeoutMs: 1_000, handshakeTimeoutMs: 1_000, pairingTimeoutMs: 1_500 },
    ...extra,
  };
}

async function connect(kind: "session" | "pairing", keys: KeyPair, extra: Partial<SessionConfig> = {}): Promise<Session> {
  const session = await connectSession(config(kind, keys, extra)).result;
  open.push(session);
  return session;
}

async function failure(kind: "session" | "pairing", keys: KeyPair, extra: Partial<SessionConfig> = {}): Promise<string> {
  try {
    const session = await connectSession(config(kind, keys, extra)).result;
    session.close();
  } catch (error) {
    expect(error).toBeInstanceOf(ConnectError);
    return (error as ConnectError).failure;
  }
  throw new Error("the connection succeeded");
}

/** Pairs a new device and returns its keys and id. */
async function pairDevice(): Promise<{ keys: KeyPair; device: string }> {
  const code = decodePairingCode(gateway.openPairing());
  const keys = generateKeyPair(realRandom);
  const session = await connect("pairing", keys, { pairingSecret: code.secret });
  session.close();
  return { keys, device: session.gateway.device };
}

function collect(session: Session): { messages: JsonValue[]; end: Promise<SessionEnd> } {
  const messages: JsonValue[] = [];
  const end = new Promise<SessionEnd>((resolve) => {
    session.listen({ onMessage: (value) => void messages.push(value), onClose: resolve });
  });
  return { messages, end };
}

describe("pairing, then a session", () => {
  it("pairs with the code, and the paired keys open a session", async () => {
    const asked: PairingRequest[] = [];
    const code = decodePairingCode(gateway.openPairing({ confirm: (request) => (asked.push(request), true) }));
    const keys = generateKeyPair(realRandom);
    const pairing = await connect("pairing", keys, { pairingSecret: code.secret });
    expect(asked.map((a) => [a.name, a.platform])).toEqual([["Test Phone", "ios"]]);
    expect(pairing.gateway).toEqual({ protocol: 1, device: "device-1", scope: "full", gateway: "Test Mac", fingerprint: gateway.fingerprint });
    expect(pairing.gatewayPublicKey).toEqual(gateway.keys.publicKey);
    pairing.close();

    const session = await connect("session", keys, { hello: { device: "device-1", name: "Test Phone", platform: "ios", app: "0.2.0" } });
    expect(session.gateway.device).toBe("device-1");
    expect(session.handshakeHash.length).toBe(32);
    expect(session.handshakeHash).not.toEqual(pairing.handshakeHash);
    expect(gateway.devices.get("device-1")?.app).toBe("0.2.0");

    const { messages } = collect(session);
    session.send({ id: 1, method: "hello", params: { client: "phone" } });
    session.send({ id: 2, method: "state", params: {} });
    await waitFor(() => messages.length === 2, "two replies");
    expect((messages[0] as JsonObject)["id"]).toBe(1);
    expect(((messages[0] as JsonObject)["result"] as JsonObject)["protocol"]).toBe(1);
    expect((messages[1] as JsonObject)["id"]).toBe(2);
  });

  it("sends the first frame of the specification: version, kind, then message 1", async () => {
    const { keys, device } = await pairDevice();
    await connect("session", keys, { hello: { device, name: "Test Phone", platform: "ios", app: "0.1.0" } });
    const [pairing, session] = sockets.connections.map((c) => c.sent[0] as Uint8Array) as [Uint8Array, Uint8Array];
    expect([pairing[0], pairing[1]]).toEqual([0x01, KIND_PAIRING]);
    expect([session[0], session[1]]).toEqual([0x01, KIND_SESSION]);
    const payload = JSON.stringify({ device, name: "Test Phone", platform: "ios", app: "0.1.0", counter: clock.now + 1 });
    // message 1 is e (32), the encrypted static key (32 + 16), the encrypted payload (+ 16).
    expect(session.length).toBe(2 + 32 + 48 + utf8Encode(payload).length + 16);
  });

  it("puts nothing of the protocol on the wire in the clear", async () => {
    const { keys, device } = await pairDevice();
    const session = await connect("session", keys, { hello: { device, name: "Test Phone", platform: "ios", app: "0.1.0" } });
    const { messages } = collect(session);
    session.send({ id: 1, method: "run.follow_up", params: { prompt: "a prompt nobody may read" }, request_id: "11111111-1111-4111-8111-111111111111" });
    await waitFor(() => messages.length === 1, "the reply");
    const wire = sockets.connections.flatMap((c) => [...c.sent, ...c.received]).map((frame) => Buffer.from(frame).toString("latin1")).join("\n");
    for (const secret of ["follow_up", "a prompt nobody may read", "Test Phone", "Test Mac", "request_id", "method", "device-1", "protocol"]) {
      expect(wire).not.toContain(secret);
    }
  });

  it("a phone without the pairing secret is refused before the owner is asked", async () => {
    let asked = 0;
    gateway.openPairing({ confirm: () => (asked++, true) });
    const keys = generateKeyPair(realRandom);
    expect(await failure("pairing", keys, { pairingSecret: realRandom(16) })).toBe("refused");
    expect(asked).toBe(0);
    expect(gateway.confirmCalls).toBe(0);
    expect(gateway.devices.size).toBe(0);
    expect(gateway.refusals).toEqual(["a wrong pairing secret"]);
  });

  it("five wrong secrets close pairing, also for the right one", async () => {
    const code = decodePairingCode(gateway.openPairing());
    for (let i = 0; i < 5; i++) expect(await failure("pairing", generateKeyPair(realRandom), { pairingSecret: realRandom(16) })).toBe("refused");
    expect(gateway.pairingOpen).toBe(false);
    expect(await failure("pairing", generateKeyPair(realRandom), { pairingSecret: code.secret })).toBe("refused");
    expect(gateway.confirmCalls).toBe(0);
  });

  it("the secret works once", async () => {
    const code = decodePairingCode(gateway.openPairing());
    await connect("pairing", generateKeyPair(realRandom), { pairingSecret: code.secret });
    expect(await failure("pairing", generateKeyPair(realRandom), { pairingSecret: code.secret })).toBe("refused");
    expect(gateway.devices.size).toBe(1);
  });

  it("a pairing the owner declines is refused, and one the owner does not answer times out", async () => {
    let code = decodePairingCode(gateway.openPairing({ confirm: () => false }));
    expect(await failure("pairing", generateKeyPair(realRandom), { pairingSecret: code.secret })).toBe("refused");
    code = decodePairingCode(gateway.openPairing({ confirm: () => new Promise<boolean>(() => undefined) }));
    expect(await failure("pairing", generateKeyPair(realRandom), { pairingSecret: code.secret, timing: { pairingTimeoutMs: 80 } })).toBe("timeout");
    expect(gateway.devices.size).toBe(0);
  });

  it("an unknown device key and a revoked device are refused", async () => {
    expect(await failure("session", generateKeyPair(realRandom))).toBe("refused");
    const { keys, device } = await pairDevice();
    await gateway.revoke(device);
    expect(await failure("session", keys)).toBe("refused");
    expect(gateway.refusals.slice(-2)).toEqual(["unknown or revoked device", "unknown or revoked device"]);
  });
});

describe("who answers", () => {
  it("nothing listening is unreachable", async () => {
    const port = gateway.port;
    await gateway.stop();
    expect(await failure("session", generateKeyPair(realRandom), { url: url(port) })).toBe("unreachable");
  });

  it("a socket that never opens is unreachable after the open timeout", async () => {
    const never: SessionConfig["socketFactory"] = () => ({ send: () => undefined, close: () => undefined });
    const started = Date.now();
    expect(await failure("session", generateKeyPair(realRandom), { socketFactory: never, timing: { openTimeoutMs: 60 } })).toBe("unreachable");
    expect(Date.now() - started).toBeLessThan(1_000);
  });

  it("a gateway with another key closes without a word: refused", async () => {
    const { keys } = await pairDevice();
    const other = new MockGateway();
    await other.start(0);
    try {
      expect(await failure("session", keys, { url: url(other.port) })).toBe("refused");
      expect(other.refusals).toEqual(["the handshake did not decrypt"]);
      expect(other.accepted).toBe(0);
    } finally {
      await other.stop();
    }
  });

  it("an impostor that answers anyway is recognised: impostor", async () => {
    const { keys } = await pairDevice();
    const impostor = new MockGateway({ answersWhatItCannotRead: true });
    await impostor.start(0);
    try {
      expect(await failure("session", keys, { url: url(impostor.port) })).toBe("impostor");
    } finally {
      await impostor.stop();
    }
  });

  it("a gateway that proves its key and speaks another protocol version is incompatible", async () => {
    const newer = new MockGateway({ protocol: 2 });
    await newer.start(0);
    try {
      const code = decodePairingCode(newer.openPairing());
      const attempt = failure("pairing", generateKeyPair(realRandom), { url: url(newer.port), gatewayPublicKey: newer.keys.publicKey, pairingSecret: code.secret });
      expect(await attempt).toBe("incompatible");
    } finally {
      await newer.stop();
    }
  });

  it("a gateway that accepts the socket and never answers times out", async () => {
    const code = decodePairingCode(gateway.openPairing({ confirm: () => new Promise<boolean>(() => undefined) }));
    expect(await failure("pairing", generateKeyPair(realRandom), { pairingSecret: code.secret, timing: { pairingTimeoutMs: 60 } })).toBe("timeout");
  });

  it("a socket factory that throws is an address that cannot be reached", async () => {
    const throwing: SessionConfig["socketFactory"] = () => {
      throw new Error("no network permission");
    };
    expect(await failure("session", generateKeyPair(realRandom), { socketFactory: throwing })).toBe("unreachable");
  });

  it("an answer without a device id is not accepted", async () => {
    const { keys } = await pairDevice();
    (gateway.devices.get("device-1") as { id: string }).id = "";
    expect(await failure("session", keys)).toBe("incompatible");
  });

  it("a cancelled attempt ends as cancelled and leaves no session behind", async () => {
    const { keys } = await pairDevice();
    const attempt = connectSession(config("session", keys));
    attempt.cancel();
    await expect(attempt.result).rejects.toMatchObject({ failure: "cancelled" });
    await sleep(30);
    expect(gateway.sessions).toBe(0);
  });

  it("a device name too long for the first frame is refused before anything is sent", async () => {
    const before = sockets.connections.length;
    expect(await failure("session", generateKeyPair(realRandom), { hello: { device: "", name: "n".repeat(5_000), platform: "ios", app: "0.1.0" } })).toBe("length");
    expect(sockets.connections.length).toBe(before);
  });
});

describe("the handshake counter", () => {
  it("a recorded first frame that is sent again is refused", async () => {
    const { keys, device } = await pairDevice();
    const hello = { device, name: "Test Phone", platform: "ios", app: "0.1.0" };
    const session = await connect("session", keys, { hello });
    const recorded = (sockets.connections.at(-1) as { sent: Uint8Array[] }).sent[0] as Uint8Array;
    session.close();
    const accepted = gateway.accepted;

    const replay = new WsWebSocket(url());
    const got: unknown[] = [];
    replay.on("message", (data) => got.push(data));
    await new Promise<void>((resolve) => replay.once("open", () => resolve()));
    replay.send(recorded);
    await new Promise<void>((resolve) => replay.once("close", () => resolve()));
    expect(got).toEqual([]);
    expect(gateway.refusals.at(-1)).toBe("a replayed handshake");
    expect(gateway.accepted).toBe(accepted);

    // The device itself is not locked out by the replay.
    await connect("session", keys, { hello });
    expect(gateway.accepted).toBe(accepted + 1);
  });

  it("is max(now, last + 1) and is in the store before the frame is sent", async () => {
    const { keys, device } = await pairDevice();
    const hello = { device, name: "Test Phone", platform: "ios", app: "0.1.0" };
    expect(store.values.get("counter")).toBe(String(clock.now));
    expect(gateway.devices.get(device)?.lastCounter).toBe(clock.now);

    const order: string[] = [];
    const set = store.set.bind(store);
    store.set = async (key, value) => {
      await set(key, value);
      order.push(`stored ${value}`);
    };
    const watching: SessionConfig["socketFactory"] = (to, handlers) => {
      const socket = sockets.factory(to, handlers);
      return { close: (code, reason) => socket.close(code, reason), send: (data) => (order.push("sent"), socket.send(data)) };
    };
    clock.now += 5_000;
    await connect("session", keys, { hello, socketFactory: watching });
    expect(order).toEqual([`stored ${clock.now}`, "sent"]);
    expect(gateway.devices.get(device)?.lastCounter).toBe(clock.now);
  });

  it("persists and increases across restarts, also when the clock goes backwards", async () => {
    const { keys, device } = await pairDevice();
    const hello = { device, name: "Test Phone", platform: "ios", app: "0.1.0" };
    const seen: number[] = [gateway.devices.get(device)?.lastCounter as number];
    const clocks = [clock.now + 10_000, clock.now - 86_400_000, clock.now - 86_400_000, 5, clock.now + 10_000, clock.now + 20_000];
    for (const now of clocks) {
      clock.now = now;
      // A restart of the app: a new counter object over the same store.
      const session = await connect("session", keys, { hello, counter: new HandshakeCounter(store, "counter", () => clock.now) });
      session.close();
      seen.push(gateway.devices.get(device)?.lastCounter as number);
      expect(store.values.get("counter")).toBe(String(seen.at(-1)));
    }
    const first = seen[0] as number;
    expect(seen).toEqual([first, first + 10_000, first + 10_001, first + 10_002, first + 10_003, first + 10_004, first + 20_000]);
    for (let i = 1; i < seen.length; i++) expect(seen[i] as number).toBeGreaterThan(seen[i - 1] as number);
  });

  it("gives two handshakes at once two different counters", async () => {
    const counter = new HandshakeCounter(new MemoryStore(), "c", () => 1_000);
    const values = await Promise.all([counter.next(), counter.next(), counter.next()]);
    expect(values).toEqual([1_000, 1_001, 1_002]);
  });

  it("sends nothing when the counter cannot be stored", async () => {
    const { keys } = await pairDevice();
    const before = sockets.connections.length;
    store.set = () => Promise.reject(new Error("the disk is full"));
    expect(await failure("session", keys)).toBe("storage");
    expect(sockets.connections.length).toBe(before);
  });
});

describe("a session", () => {
  async function session(extra: Partial<SessionConfig> = {}): Promise<Session> {
    const { keys, device } = await pairDevice();
    return connect("session", keys, { hello: { device, name: "Test Phone", platform: "ios", app: "0.1.0" }, ...extra });
  }

  it("joins a message of several frames into one value", async () => {
    const s = await session();
    const { messages } = collect(s);
    gateway.emit({ text: "x".repeat(200_000) });
    s.send({ id: 1, method: "events.subscribe", params: { after: 0 } });
    await waitFor(() => messages.length === 3, "reply, event, replayed");
    const event = (messages[1] as JsonObject)["params"] as JsonObject;
    expect(((event["payload"] as JsonObject)["text"] as string).length).toBe(200_000);
    expect((sockets.connections.at(-1) as { received: Uint8Array[] }).received.length).toBeGreaterThanOrEqual(1 + 1 + 4 + 1);
  });

  it("delivers messages one at a time, in order, waiting for the handler", async () => {
    const s = await session();
    for (let i = 0; i < 5; i++) gateway.emit({ n: i });
    const order: string[] = [];
    let busy = false;
    s.listen({
      onMessage: async (value) => {
        expect(busy).toBe(false);
        busy = true;
        const seq = (((value as JsonObject)["params"] ?? {}) as JsonObject)["seq"];
        order.push(`start ${String(seq)}`);
        await sleep(3);
        order.push(`end ${String(seq)}`);
        busy = false;
      },
      onClose: () => undefined,
    });
    s.send({ id: 1, method: "events.subscribe", params: { after: 0 } });
    await waitFor(() => order.length === 14, "every message handled");
    expect(order.slice(2, 12)).toEqual([1, 2, 3, 4, 5].flatMap((n) => [`start ${n}`, `end ${n}`]));
  });

  it("keeps what arrived before anyone listened", async () => {
    const s = await session();
    s.send({ id: 7, method: "state", params: {} });
    await sleep(40);
    const { messages } = collect(s);
    await waitFor(() => messages.length === 1, "the kept reply");
    expect((messages[0] as JsonObject)["id"]).toBe(7);
  });

  it("refuses a request over 1 MiB and stays open", async () => {
    const s = await session();
    const { messages } = collect(s);
    expect(() => s.send({ id: 1, method: "state", params: { big: "x".repeat(1024 * 1024) } })).toThrowError(/1 MiB/);
    s.send({ id: 2, method: "state", params: { big: "x".repeat(1024 * 1024 - 200) } });
    await waitFor(() => messages.length === 1, "the reply to the large request");
    expect(s.closed).toBe(false);
  });

  it("ends when the gateway goes away, after delivering what arrived", async () => {
    const s = await session();
    const { messages, end } = collect(s);
    s.send({ id: 1, method: "state", params: {} });
    await waitFor(() => messages.length === 1, "the reply");
    gateway.dropAll();
    expect((await end).reason).toBe("closed_by_peer");
    expect(s.closed).toBe(true);
    expect(() => s.send({ id: 2, method: "state", params: {} })).toThrowError(/closed/);
  });

  it("ends on a frame that does not decrypt, on a bad flag and on what is not JSON", async () => {
    type Tamper = (frame: Uint8Array) => Uint8Array;
    const cases: [string, Tamper][] = [
      ["decrypt", (frame) => frame.map((b, i) => (i === 5 ? b ^ 1 : b))],
      ["length", () => new Uint8Array(3)],
    ];
    for (const [detail, tamper] of cases) {
      let sawReply = false;
      const tampering: SessionConfig["socketFactory"] = (to, handlers) =>
        sockets.factory(to, {
          ...handlers,
          onMessage: (data) => {
            // Message 2 passes; the first transport frame is changed.
            if (!sawReply) {
              sawReply = true;
              handlers.onMessage(data);
            } else handlers.onMessage(tamper(data));
          },
        });
      const s = await session({ socketFactory: tampering });
      const { messages, end } = collect(s);
      s.send({ id: 1, method: "state", params: {} });
      const ended = await end;
      expect([ended.reason, ended.detail]).toEqual(["protocol", detail]);
      expect(messages).toEqual([]);
    }
  });

  it("ends on a replayed and on a reordered frame", async () => {
    for (const mode of ["replay", "reorder"] as const) {
      let count = 0;
      let held: Uint8Array | null = null;
      const meddling: SessionConfig["socketFactory"] = (to, handlers) =>
        sockets.factory(to, {
          ...handlers,
          onMessage: (data) => {
            count += 1;
            if (count === 1) return handlers.onMessage(data);
            if (mode === "replay") {
              handlers.onMessage(data);
              handlers.onMessage(data);
            } else if (held === null) {
              held = data;
            } else {
              handlers.onMessage(data);
              handlers.onMessage(held);
            }
          },
        });
      const s = await session({ socketFactory: meddling });
      const { messages, end } = collect(s);
      s.send({ id: 1, method: "state", params: {} });
      s.send({ id: 2, method: "state", params: {} });
      const ended = await end;
      expect([mode, ended.reason, ended.detail]).toEqual([mode, "protocol", "decrypt"]);
      expect(messages.length).toBe(mode === "replay" ? 1 : 0);
    }
  });

  it("ends when the gateway sends what is not JSON, and when it sends text", async () => {
    const s = await session();
    const { end } = collect(s);
    s.send({ id: 1, method: "state", params: {} });
    // The gateway's own cipher, used to send a message that is not JSON.
    const internals = gateway as unknown as { connections: Set<{ transport: { send: { encrypt(p: Uint8Array): Uint8Array } }; ws: { send(d: Uint8Array | string): void } }> };
    const [connection] = [...internals.connections];
    connection?.ws.send(connection.transport.send.encrypt(utf8Encode("\u0001{not json")));
    const ended = await end;
    expect([ended.reason, ended.detail]).toEqual(["protocol", "not JSON"]);

    const s2 = await session();
    const second = collect(s2);
    const [other] = [...internals.connections];
    other?.ws.send("text is not allowed");
    expect((await second.end).reason).toBe("closed_by_peer");
  });

  it("sends a keepalive on time, hides its reply, and uses the socket's ping where there is one", async () => {
    const pinging = new RecordingSockets(wsSockets);
    const s = await session({ socketFactory: pinging.factory, timing: { keepaliveMs: 25, idleTimeoutMs: 500 } });
    const { messages } = collect(s);
    await waitFor(() => gateway.received.filter((r) => r.method === "ping").length >= 3, "three keepalives");
    expect(gateway.received.filter((r) => r.method === "ping").every((r) => r.id === 0)).toBe(true);
    expect(messages).toEqual([]);
    expect(s.closed).toBe(false);
    expect((pinging.connections.at(-1) as { pings: number }).pings).toBeGreaterThanOrEqual(3);

    // Node's global WebSocket cannot ping; the keepalive request alone keeps the session.
    const plain = await session({ timing: { keepaliveMs: 25, idleTimeoutMs: 500 } });
    const before = gateway.received.length;
    await waitFor(() => gateway.received.length >= before + 2, "keepalives without ping");
    expect((sockets.connections.at(-1) as { pings: number }).pings).toBe(0);
    expect(plain.closed).toBe(false);
  });

  it("ends as idle when the gateway falls silent", async () => {
    let silent = false;
    const muting: SessionConfig["socketFactory"] = (to, handlers) =>
      sockets.factory(to, { ...handlers, onMessage: (data) => (silent ? undefined : handlers.onMessage(data)) });
    const s = await session({ socketFactory: muting, timing: { keepaliveMs: 20, idleTimeoutMs: 60 } });
    const { end } = collect(s);
    await sleep(70);
    expect(s.closed).toBe(false);
    silent = true;
    const ended = await end;
    expect(ended.reason).toBe("idle");
  });

  it("a probe passes on a live session and ends a dead one quickly", async () => {
    let silent = false;
    const muting: SessionConfig["socketFactory"] = (to, handlers) =>
      sockets.factory(to, { ...handlers, onMessage: (data) => (silent ? undefined : handlers.onMessage(data)) });
    const s = await session({ socketFactory: muting });
    const { end } = collect(s);
    s.probe(100);
    await sleep(150);
    expect(s.closed).toBe(false);
    silent = true;
    const started = Date.now();
    s.probe(60);
    expect((await end).reason).toBe("idle");
    expect(Date.now() - started).toBeLessThan(1_000);
  });

  it("reports the time of every genuine frame, and of nothing else", async () => {
    const heard: number[] = [];
    const s = await session({ onActivity: (at) => heard.push(at) });
    collect(s);
    const afterHandshake = heard.length;
    expect(afterHandshake).toBeGreaterThanOrEqual(1);
    clock.now += 1_234;
    s.send({ id: 1, method: "state", params: {} });
    await waitFor(() => heard.length > afterHandshake, "a frame");
    expect(heard.at(-1)).toBe(clock.now);
  });
});
