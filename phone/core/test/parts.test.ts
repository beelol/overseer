import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { candidateAddresses, formatAddress, gatewayUrl, isUsableAddress, parseAddress } from "../src/addresses.ts";
import { Backoff } from "../src/backoff.ts";
import { METHOD_CLASS, PROTOCOL_VERSION as GENERATED_VERSION } from "../../protocol/protocol.generated.ts";
import { PROTOCOL_VERSION } from "../src/session.ts";
import { Emitter } from "../src/emitter.ts";
import { Outbox } from "../src/outbox.ts";
import { PairingStore } from "../src/pairing-store.ts";
import { generateKeyPair } from "../src/noise.ts";
import { MemoryStore, randomFraction } from "../src/platform.ts";
import { SavedValue } from "../src/saved-value.ts";
import { isRequestId, uuidV4 } from "../src/uuid.ts";
import { realRandom, RecordingStore, seededRandom, sleep } from "./helpers.ts";

describe("addresses", () => {
  it("makes the gateway's URL, with brackets around IPv6 and an escaped zone", () => {
    expect(gatewayUrl({ host: "192.168.1.20", port: 47_810 })).toBe("ws://192.168.1.20:47810/v1");
    expect(gatewayUrl({ host: "bilals-mac.local", port: 1 })).toBe("ws://bilals-mac.local:1/v1");
    expect(gatewayUrl({ host: "::1", port: 47_810 })).toBe("ws://[::1]:47810/v1");
    expect(gatewayUrl({ host: "fe80::1%en0", port: 47_810 })).toBe("ws://[fe80::1%25en0]:47810/v1");
    expect(new URL(gatewayUrl({ host: "fd12:3456::1", port: 8 })).port).toBe("8");
    expect(formatAddress({ host: "::1", port: 5 })).toBe("[::1]:5");
    expect(formatAddress({ host: "10.0.2.2", port: 5 })).toBe("10.0.2.2:5");
  });

  it("orders the candidates and drops repeats and what is not an address", () => {
    const list = candidateAddresses({
      last: { host: "10.0.0.5", port: 47_810 },
      discovered: [{ host: "10.0.0.9", port: 5_000 }, { host: "10.0.0.5", port: 47_810 }, { host: "bad host" }],
      paired: ["192.168.1.20", "10.0.0.5", "MAC.local", ""],
      extras: [{ host: "127.0.0.1" }, { host: "mac.local" }, { host: "10.0.2.2", port: 70_000 }],
      port: 47_810,
    });
    expect(list.map(formatAddress)).toEqual(["10.0.0.5:47810", "10.0.0.9:5000", "192.168.1.20:47810", "MAC.local:47810", "127.0.0.1:47810"]);
    expect(candidateAddresses({ last: null, discovered: [], paired: [], extras: [], port: 1 })).toEqual([]);
  });

  it("reads an address a person typed", () => {
    expect(parseAddress(" 192.168.1.20 ")).toEqual({ host: "192.168.1.20", port: 47_810 });
    expect(parseAddress("192.168.1.20:5000")).toEqual({ host: "192.168.1.20", port: 5_000 });
    expect(parseAddress("mac.local", 9)).toEqual({ host: "mac.local", port: 9 });
    expect(parseAddress("[fe80::1%en0]:5000")).toEqual({ host: "fe80::1%en0", port: 5_000 });
    expect(parseAddress("[::1]")).toEqual({ host: "::1", port: 47_810 });
    expect(parseAddress("fd12:3456::1")).toEqual({ host: "fd12:3456::1", port: 47_810 });
    for (const bad of ["", "   ", "a b", "host:port", "host:0", "host:65536", "ws://host", "host/path", "[::1", "[::1]:x"]) {
      expect(() => parseAddress(bad), bad).toThrowError(/not an address/);
    }
    expect(isUsableAddress({ host: "x".repeat(256) })).toBe(false);
    expect(isUsableAddress({ host: "ok", port: 1.5 })).toBe(false);
  });
});

describe("backoff", () => {
  it("doubles from the minimum to the maximum, each wait between half the step and the step", () => {
    const backoff = new Backoff(500, 10_000, seededRandom(3));
    const steps = [500, 1_000, 2_000, 4_000, 8_000, 10_000, 10_000];
    for (const step of steps) {
      expect(backoff.current).toBe(step);
      const wait = backoff.next();
      expect(wait).toBeGreaterThanOrEqual(step / 2);
      expect(wait).toBeLessThanOrEqual(step);
    }
    backoff.reset();
    expect(backoff.current).toBe(500);
  });

  it("uses the whole range of the jitter", () => {
    expect(new Backoff(500, 500, () => Uint8Array.of(0, 0, 0, 0)).next()).toBe(250);
    expect(new Backoff(500, 500, () => Uint8Array.of(255, 255, 255, 255)).next()).toBeCloseTo(500, 3);
    expect(randomFraction(() => Uint8Array.of(128, 0, 0, 0))).toBe(0.5);
    expect(() => randomFraction(() => new Uint8Array(3))).toThrowError();
    const waits = new Set(Array.from({ length: 50 }, () => new Backoff(500, 500, realRandom).next()));
    expect(waits.size).toBeGreaterThan(40);
  });
});

describe("request ids", () => {
  it("are version 4 UUIDs that the gateway accepts, different every time", () => {
    const ids = new Set(Array.from({ length: 500 }, () => uuidV4(realRandom)));
    expect(ids.size).toBe(500);
    for (const id of ids) {
      expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
      expect(isRequestId(id)).toBe(true);
    }
    expect(() => uuidV4(() => new Uint8Array(15))).toThrowError();
    for (const bad of ["", "short", "x".repeat(65), "has space 12345", "under_score_12345", "émoji-1234567"]) expect(isRequestId(bad), bad).toBe(false);
  });
});

describe("the protocol description", () => {
  it("is the one the generated types were made from", () => {
    const document = JSON.parse(readFileSync(new URL("../../../protocol/protocol.json", import.meta.url), "utf8")) as { version: number; methods: Record<string, { class: string }> };
    const described = Object.fromEntries(Object.entries(document.methods).map(([name, method]) => [name, method.class]));
    expect(METHOD_CLASS).toEqual(described);
    expect(PROTOCOL_VERSION).toBe(document.version);
    expect(PROTOCOL_VERSION).toBe(GENERATED_VERSION);
    expect([METHOD_CLASS["hello"], METHOD_CLASS["ping"], METHOD_CLASS["events.subscribe"]]).toEqual(["self", "self", "self"]);
    expect([METHOD_CLASS["run.follow_up"], METHOD_CLASS["run.permission"], METHOD_CLASS["state"]]).toEqual(["control", "control", "read"]);
  });
});

describe("the emitter", () => {
  it("calls listeners in order, survives one that throws, and removes on request", async () => {
    const logs: string[] = [];
    const emitter = new Emitter<{ thing: (n: number) => void | Promise<void> }>((line) => logs.push(line));
    const seen: string[] = [];
    emitter.on("thing", (n) => void seen.push(`a${n}`));
    emitter.on("thing", () => {
      throw new Error("a listener with a bug");
    });
    emitter.on("thing", () => Promise.reject(new Error("a listener that rejects")));
    const off = emitter.on("thing", (n) => void seen.push(`b${n}`));
    emitter.emit("thing", 1);
    off();
    emitter.emit("thing", 2);
    await emitter.emitAndWait("thing", 3);
    await sleep(5);
    expect(seen).toEqual(["a1", "b1", "a2", "a3"]);
    expect(logs.length).toBe(6);
    expect(logs.join(" ")).not.toContain("bug");
  });

  it("waits for each listener before the next", async () => {
    const emitter = new Emitter<{ thing: () => Promise<void> }>();
    const order: string[] = [];
    emitter.on("thing", async () => {
      order.push("first starts");
      await sleep(5);
      order.push("first ends");
    });
    emitter.on("thing", async () => void order.push("second"));
    await emitter.emitAndWait("thing");
    expect(order).toEqual(["first starts", "first ends", "second"]);
  });
});

describe("a saved value", () => {
  it("writes the newest value, never an older one after it, never two at once", async () => {
    const store = new RecordingStore();
    store.delayMs = 3;
    let writing = 0;
    let most = 0;
    const set = store.set.bind(store);
    store.set = async (key, value) => {
      writing += 1;
      most = Math.max(most, writing);
      await set(key, value);
      writing -= 1;
    };
    const saved = new SavedValue(store, "cursor");
    for (let i = 1; i <= 200; i++) saved.set(String(i));
    await saved.flush();
    expect(store.values.get("cursor")).toBe("200");
    expect(most).toBe(1);
    const written = store.writes.map((w) => Number(w.value));
    expect(written.length).toBeLessThan(10);
    expect([...written].sort((a, b) => a - b)).toEqual(written);
    saved.set(null);
    await saved.flush();
    expect(store.values.has("cursor")).toBe(false);
    expect(await saved.load()).toBeNull();
  });

  it("goes on after a store that failed", async () => {
    const store = new RecordingStore();
    const set = store.set.bind(store);
    let fail = true;
    store.set = (key, value) => (fail ? Promise.reject(new Error("full")) : set(key, value));
    const logs: string[] = [];
    const saved = new SavedValue(store, "k", (line) => logs.push(line));
    saved.set("1");
    await saved.flush();
    fail = false;
    saved.set("2");
    await saved.flush();
    expect(store.values.get("k")).toBe("2");
    expect(logs).toEqual(["could not store k"]);
  });
});

describe("the outbox", () => {
  it("keeps the order, stores only what is unanswered, and reads it back as queued", async () => {
    const store = new MemoryStore();
    const outbox = new Outbox(store, "outbox");
    await outbox.load();
    for (const n of [1, 2, 3]) outbox.add({ requestId: `request-${n}`, method: "run.follow_up", params: { n }, createdAt: n });
    outbox.markSending(["request-1", "request-2"], 100);
    outbox.finish("request-1", { result: { ok: true } });
    await outbox.save();
    expect(outbox.entries().map((e) => `${e.requestId} ${e.state}`)).toEqual(["request-2 sending", "request-3 queued", "request-1 done"]);

    const again = new Outbox(store, "outbox");
    await again.load();
    expect(again.entries().map((e) => [e.requestId, e.state, e.attempts, e.firstSentAt, e.params])).toEqual([
      ["request-2", "queued", 1, 100, { n: 2 }],
      ["request-3", "queued", 0, null, { n: 3 }],
    ]);
    again.finish("request-2", { error: { code: "failed", message: "no" } });
    again.finish("request-3", { result: null });
    expect(again.finish("request-3", { result: null })).toBeNull();
    await again.save();
    expect(await store.get("outbox")).toBeNull();
  });

  it("starts empty from a store that holds something else", async () => {
    const store = new MemoryStore();
    for (const bad of ["not json", "{}", "[1, null, {\"requestId\": 5}, {\"requestId\": \"a\", \"method\": \"m\"}]"]) {
      await store.set("outbox", bad);
      const outbox = new Outbox(store, "outbox");
      await outbox.load();
      expect(outbox.entries()).toEqual([]);
    }
  });

  it("keeps only the latest answered entries in memory", async () => {
    const outbox = new Outbox(new MemoryStore(), "outbox");
    for (let n = 0; n < 60; n++) {
      outbox.add({ requestId: `request-${n}`, method: "m", params: {}, createdAt: n });
      outbox.finish(`request-${n}`, { result: n });
    }
    expect(outbox.entries().length).toBe(50);
    expect(outbox.entries()[0]?.requestId).toBe("request-10");
  });
});

describe("the pairing store", () => {
  const device = generateKeyPair(realRandom);
  const gateway = generateKeyPair(realRandom);
  const pairing = {
    deviceId: "d-1",
    deviceName: "Bilal's iPhone",
    platform: "ios",
    gatewayName: "Mac",
    scope: "full",
    gatewayPublicKey: gateway.publicKey,
    devicePrivateKey: device.privateKey,
    devicePublicKey: device.publicKey,
    port: 47_810,
    addresses: ["192.168.1.20"],
    pairedAt: 5,
  };

  it("round trips, with the keys in the secret store only", async () => {
    const store = new MemoryStore();
    const secrets = new MemoryStore();
    const pairings = new PairingStore(store, secrets, "overseer.");
    expect(await pairings.load()).toBeNull();
    await pairings.save(pairing);
    expect(await pairings.load()).toEqual(pairing);
    expect(store.keys()).toEqual(["overseer.pairing"]);
    expect(secrets.keys()).toEqual(["overseer.keys"]);
    expect(await store.get("overseer.pairing")).not.toMatch(/[0-9a-f]{64}/);
    expect((await secrets.get("overseer.keys"))?.length).toBeLessThan(2_000);
    await pairings.forget();
    expect([store.keys(), secrets.keys()]).toEqual([[], []]);
  });

  it("is no pairing when a part is missing, damaged or does not fit", async () => {
    const damage: ((store: MemoryStore, secrets: MemoryStore) => Promise<void>)[] = [
      (store) => store.delete("overseer.pairing"),
      (_store, secrets) => secrets.delete("overseer.keys"),
      (store) => store.set("overseer.pairing", "not json"),
      (_store, secrets) => secrets.set("overseer.keys", "{}"),
      async (_store, secrets) => {
        const keys = JSON.parse((await secrets.get("overseer.keys")) as string) as Record<string, string>;
        // A private key that is not the one of the public key.
        await secrets.set("overseer.keys", JSON.stringify({ ...keys, devicePrivateKey: "11".repeat(32) }));
      },
      async (store) => store.set("overseer.pairing", JSON.stringify({ v: 2, deviceId: "d-1", port: 1, addresses: [] })),
    ];
    for (const harm of damage) {
      const store = new MemoryStore();
      const secrets = new MemoryStore();
      const pairings = new PairingStore(store, secrets, "overseer.");
      await pairings.save(pairing);
      await harm(store, secrets);
      expect(await pairings.load()).toBeNull();
    }
  });
});
