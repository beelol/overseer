import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { PairingError } from "../src/client.ts";
import { CONNECTION_STATES, type ForgetReason } from "../src/client-types.ts";
import { decodePairingCode, encodePairingCode } from "../src/pairing-code.ts";
import { type Harness, newClient, newPhone, type Phone, sleep, untilEntered, untilState, waitFor } from "./helpers.ts";
import { MockGateway } from "./mock-gateway.ts";

let gateway: MockGateway;
let phone: Phone;
const running: Harness[] = [];
const gateways: MockGateway[] = [];

beforeEach(async () => {
  gateway = new MockGateway();
  gateways.push(gateway);
  await gateway.start(0);
  phone = newPhone();
});

afterEach(async () => {
  for (const h of running.splice(0)) await h.client.stop();
  for (const g of gateways.splice(0)) await g.stop();
});

function client(overrides: Parameters<typeof newClient>[1] = {}): Harness {
  const h = newClient(phone, overrides);
  running.push(h);
  return h;
}

async function paired(overrides: Parameters<typeof newClient>[1] = {}): Promise<Harness> {
  const h = client(overrides);
  await h.client.start();
  await h.client.pair(gateway.openPairing(), "Test Phone", "ios");
  await untilState(h, "online");
  return h;
}

describe("the states", () => {
  it("are exactly seven", () => {
    expect([...CONNECTION_STATES]).toEqual(["unpaired", "connecting", "online", "reconnecting", "off", "unreachable", "revoked"]);
  });

  it("unpaired without a pairing: nothing is tried", async () => {
    const h = client();
    expect(h.client.state).toBe("unpaired");
    await h.client.start();
    h.client.wake();
    await sleep(40);
    expect(h.client.state).toBe("unpaired");
    expect(h.client.gateway).toBeNull();
    expect(phone.sockets.connections).toEqual([]);
    await expect(h.client.request("state", {})).rejects.toMatchObject({ code: "unpaired" });
    await expect(h.client.request("run.follow_up", { run_id: "run-1", prompt: "x" })).rejects.toMatchObject({ code: "unpaired" });
  });

  it("connecting, then online, after pairing; and what the app may show about the Mac", async () => {
    const h = await paired();
    expect(h.states).toEqual(["connecting", "online"]);
    expect(h.client.gateway).toMatchObject({ deviceId: "device-1", deviceName: "Test Phone", platform: "ios", gatewayName: "Test Mac", gatewayFingerprint: gateway.fingerprint, scope: "full" });
    expect(h.client.hello).toMatchObject({ protocol: 1, device: { scope: "full" } });
  });

  it("off notice, then off; the gateway comes back and it is online by itself, with no pairing", async () => {
    const h = await paired();
    gateway.emit({ n: 1 });
    await waitFor(() => h.seqs.length === 1, "the event");

    await gateway.turnOff();
    await untilState(h, "off");
    // It retries quietly, and stays off while nothing answers.
    const tries = phone.sockets.connections.length;
    await waitFor(() => phone.sockets.connections.length >= tries + 3, "quiet retries");
    expect(h.client.state).toBe("off");
    expect(h.states).toEqual(["connecting", "online", "off"]);
    await expect(h.client.request("state", {})).rejects.toMatchObject({ code: "not_connected" });

    gateway.emit({ n: 2 });
    await gateway.turnOn();
    await untilState(h, "online");
    await waitFor(() => h.seqs.length === 2, "what happened meanwhile");
    expect(h.states).toEqual(["connecting", "online", "off", "online"]);
    expect(h.seqs).toEqual([1, 2]);
    expect(h.pairCalls).toBe(1);
    expect(phone.sockets.handshakes(2)).toBe(1);
    expect(gateway.pairingHandshakes).toBe(1);
  });

  it("while off it retries at the slow interval, and at once when the app wakes", async () => {
    const h = await paired({ timing: { ...newTiming(), offRetryMs: 60_000 } });
    await gateway.turnOff();
    await untilState(h, "off");
    await sleep(60);
    const tries = phone.sockets.connections.length;
    await sleep(60);
    expect(phone.sockets.connections.length).toBe(tries);
    await gateway.turnOn();
    h.client.wake();
    await untilState(h, "online", 2_000);
    expect(h.states.slice(-2)).toEqual(["off", "online"]);
  });

  it("revoke, then revoked, then unpaired: the keys are deleted and pairing is offered again", async () => {
    const forgotten: ForgetReason[] = [];
    const h = await paired({ onForget: (reason) => void forgotten.push(reason) });
    void h.client.requestRaw("run.follow_up", { never: "answered" }, { requestId: "kept-until-revoked" }).catch(() => undefined);
    expect(phone.secrets.keys()).toEqual(["overseer.keys"]);
    expect(phone.store.keys()).toContain("overseer.pairing");

    await gateway.revoke("device-1");
    await untilState(h, "unpaired");
    expect(h.states).toEqual(["connecting", "online", "revoked", "unpaired"]);
    expect(forgotten).toEqual(["revoked"]);
    expect(phone.secrets.keys()).toEqual([]);
    // Only the handshake counter stays: it must never go back.
    expect(phone.store.keys()).toEqual(["overseer.counter"]);
    expect(h.client.gateway).toBeNull();
    expect(h.client.lastContact).toBeNull();
    expect(h.client.cursor).toBe(0);

    // Nothing is tried any more, and a restart of the app finds no pairing.
    const tries = phone.sockets.connections.length;
    h.client.wake();
    await sleep(60);
    expect(phone.sockets.connections.length).toBe(tries);
    const second = client();
    await second.client.start();
    expect(second.client.state).toBe("unpaired");

    // The old key never works again; a new pairing does.
    await second.client.pair(gateway.openPairing(), "Test Phone", "ios");
    await untilState(second, "online");
    expect(second.client.gateway?.deviceId).toBe("device-2");
  });

  it("a reply that says revoked has the same effect as the notice", async () => {
    const h = await paired();
    await waitFor(() => gateway.liveSubscribers === 1, "the subscription");
    (gateway.devices.get("device-1") as { revoked: boolean }).revoked = true;
    const first = h.client.request("state", {});
    const second = h.client.request("state", {});
    await expect(first).rejects.toMatchObject({ code: "revoked" });
    // What was still waiting for a reply hears why it will never get one.
    await expect(second).rejects.toMatchObject({ code: "unpaired", message: "this phone was revoked on the Mac" });
    await untilState(h, "unpaired");
    expect(h.states.slice(-2)).toEqual(["revoked", "unpaired"]);
    expect(phone.secrets.keys()).toEqual([]);
  });

  it("requests that waited in the outbox fail when the device is revoked", async () => {
    const h = await paired();
    await gateway.stop();
    await untilState(h, "unreachable");
    const waiting = h.client.request("run.follow_up", { run_id: "run-1", prompt: "never sent" });
    await gateway.start();
    (gateway.devices.get("device-1") as { revoked: boolean }).revoked = true;
    // Revoked while away: the gateway closes without a word, and the app cannot know why.
    await waitFor(() => h.attempts.filter((a) => a.outcome === "refused").length >= 2, "refusals");
    expect(h.client.state).toBe("unreachable");
    expect(phone.secrets.keys()).toEqual(["overseer.keys"]);
    await h.client.forget();
    await expect(waiting).rejects.toMatchObject({ code: "unpaired" });
    expect(h.client.state).toBe("unpaired");
    expect(gateway.executions).toBe(0);
  });

  it("no candidate answers: unreachable, with the last contact kept, and it keeps retrying", async () => {
    const clock = { now: 1_790_000_000_000 };
    const h = await paired({ now: () => clock.now });
    clock.now += 60_000;
    gateway.emit({ n: 1 });
    await waitFor(() => h.seqs.length === 1, "the event");
    const contact = clock.now;
    expect(h.client.lastContact).toBe(contact);

    clock.now += 60_000;
    await gateway.stop();
    await untilState(h, "unreachable");
    expect(h.states).toEqual(["connecting", "online", "reconnecting", "unreachable"]);
    expect(h.client.lastContact).toBe(contact);
    expect(h.attempts.at(-1)).toMatchObject({ outcome: "unreachable", address: { host: "127.0.0.1", port: gateway.port } });

    const tries = phone.sockets.connections.length;
    await waitFor(() => phone.sockets.connections.length >= tries + 3, "more tries");
    expect(h.client.state).toBe("unreachable");
    expect(h.states.filter((s) => s === "unreachable").length).toBe(1);

    // The app is closed and opened again while the Mac is away.
    await h.client.stop();
    clock.now += 3_600_000;
    const second = client({ now: () => clock.now });
    await second.client.start();
    await untilState(second, "unreachable");
    expect(second.states).toEqual(["connecting", "unreachable"]);
    expect(second.client.lastContact).toBe(contact);

    await gateway.start();
    await untilState(second, "online");
    expect(second.client.lastContact).toBe(clock.now);
    expect(second.pairCalls).toBe(0);
  });

  it("tries the addresses in order: the last that worked, discovered, the pairing code's, the platform's", async () => {
    const h = client({ extras: [{ host: "127.0.0.4" }, { host: "127.0.0.1" }] });
    await h.client.start();
    await h.client.pair(gateway.openPairing({ addresses: ["127.0.0.1", "127.0.0.3"] }), "Test Phone", "ios");
    await untilState(h, "online");
    h.client.setDiscovered([{ host: "127.0.0.2", port: gateway.port }, { host: "not a host" }, { host: "127.0.0.1", port: gateway.port }]);
    await gateway.stop();
    await untilState(h, "unreachable");
    const pass = h.attempts.slice(-4).map((a) => `${a.address.host}:${a.address.port} ${a.outcome}`);
    const p = gateway.port;
    expect(pass).toEqual([`127.0.0.1:${p} unreachable`, `127.0.0.2:${p} unreachable`, `127.0.0.3:${p} unreachable`, `127.0.0.4:${p} unreachable`]);
  });

  it("an impostor with another key at the first address is refused, and the second address works", async () => {
    const h = await paired();
    const oldPort = gateway.port;
    // The Mac moved; at its old address something else answers now, with another key.
    await gateway.stop();
    await untilState(h, "unreachable");
    const impostor = new MockGateway({ name: "Test Mac", answersWhatItCannotRead: true });
    gateways.push(impostor);
    await impostor.start(oldPort);
    await gateway.start(0);
    h.client.setDiscovered([{ host: "127.0.0.1", port: gateway.port }]);

    await untilState(h, "online");
    expect(h.attempts.slice(-2).map((a) => `${a.address.port} ${a.outcome}`)).toEqual([`${oldPort} impostor`, `${gateway.port} connected`]);
    expect(impostor.accepted).toBe(0);
    expect(impostor.received).toEqual([]);
    expect(h.pairCalls).toBe(1);
    expect(await h.client.request("state", {})).toMatchObject({ cursor: 0 });

    // Next time the address that worked is tried first.
    gateway.dropAll();
    await untilEntered(h, "online", 3);
    expect(h.attempts.at(-1)).toMatchObject({ outcome: "connected", address: { port: gateway.port } });
    expect(h.attempts.filter((a) => a.outcome === "impostor").length).toBe(1);
  });

  it("a Mac with another key that closes without a word is passed over too", async () => {
    const h = await paired();
    const oldPort = gateway.port;
    await gateway.stop();
    await untilState(h, "unreachable");
    const other = new MockGateway({ name: "Test Mac" });
    gateways.push(other);
    await other.start(oldPort);
    await gateway.start(0);
    h.client.setDiscovered([{ host: "127.0.0.1", port: gateway.port }]);
    await untilState(h, "online");
    expect(h.attempts.slice(-2).map((a) => `${a.address.port} ${a.outcome}`)).toEqual([`${oldPort} refused`, `${gateway.port} connected`]);
    expect(other.refusals).toContain("the handshake did not decrypt");
  });

  it("backs off from the minimum, doubling to the maximum, and starts over after a success", async () => {
    // Every other wait has a value of its own here, so the waits between passes can be told apart.
    const timing = { ...newTiming(), backoffMinMs: 500, backoffMaxMs: 10_000, openTimeoutMs: 1_111, handshakeTimeoutMs: 2_222, pairingTimeoutMs: 3_333, requestTimeoutMs: 4_444, keepaliveMs: 55_555 };
    const others = new Set([1_111, 2_222, 3_333, 4_444, 55_555]);
    const waits: number[] = [];
    const timers = {
      set: (callback: () => void, ms: number) => {
        if (others.has(ms)) return setTimeout(callback, ms);
        waits.push(ms);
        // The wait is recorded as asked for, and shortened so that the test does not take a minute.
        return setTimeout(callback, 3);
      },
      clear: (handle: unknown) => clearTimeout(handle as NodeJS.Timeout),
    };
    const h = await paired({ timers, timing });
    expect(waits).toEqual([]);
    await gateway.stop();
    await waitFor(() => waits.length >= 9, "nine waits");
    const steps = [500, 1_000, 2_000, 4_000, 8_000, 10_000, 10_000, 10_000, 10_000];
    const seen = waits.slice(0, 9);
    seen.forEach((ms, i) => {
      expect(ms, `wait ${i}`).toBeGreaterThanOrEqual((steps[i] as number) / 2);
      expect(ms, `wait ${i}`).toBeLessThanOrEqual(steps[i] as number);
    });
    // Jitter: the waits at the maximum are not all the same.
    expect(new Set(seen.slice(5)).size).toBeGreaterThan(1);

    await gateway.start();
    await untilState(h, "online");
    await waitFor(() => gateway.liveSubscribers === 1, "the subscription");
    waits.length = 0;
    await gateway.stop();
    await waitFor(() => waits.length >= 2, "the waits after the success");
    expect(waits[0]).toBeGreaterThanOrEqual(250);
    expect(waits[0]).toBeLessThanOrEqual(500);
    expect(waits[1]).toBeGreaterThanOrEqual(500);
    expect(waits[1]).toBeLessThanOrEqual(1_000);
  });

  it("a gateway that greets and then refuses the subscription is retried ever more slowly", async () => {
    const timing = { ...newTiming(), backoffMinMs: 500, backoffMaxMs: 10_000, openTimeoutMs: 1_111, handshakeTimeoutMs: 2_222, pairingTimeoutMs: 3_333, requestTimeoutMs: 4_444, keepaliveMs: 55_555 };
    const others = new Set([1_111, 2_222, 3_333, 4_444, 55_555]);
    const waits: number[] = [];
    const timers = {
      set: (callback: () => void, ms: number) => {
        if (others.has(ms)) return setTimeout(callback, ms);
        waits.push(ms);
        return setTimeout(callback, 3);
      },
      clear: (handle: unknown) => clearTimeout(handle as NodeJS.Timeout),
    };
    const h = await paired({ timers, timing });
    await waitFor(() => gateway.liveSubscribers === 1, "the subscription");
    // From now on the device may not subscribe: it is only allowed to watch nothing at all.
    const internals = gateway as unknown as { subscribe: (...args: unknown[]) => Promise<void>; send: (c: unknown, value: unknown) => void };
    internals.subscribe = async (c, id) => internals.send(c, { id, error: { code: "failed", message: "no subscription today" } });
    gateway.dropAll();
    await waitFor(() => waits.length >= 5, "five waits");
    expect(h.states.filter((s) => s === "online").length).toBeGreaterThanOrEqual(4);
    const steps = [500, 1_000, 2_000, 4_000, 8_000];
    waits.slice(0, 5).forEach((ms, i) => {
      expect(ms, `wait ${i}`).toBeGreaterThanOrEqual((steps[i] as number) / 2);
      expect(ms, `wait ${i}`).toBeLessThanOrEqual(steps[i] as number);
    });
  });

  it("wake retries at once", async () => {
    const h = await paired({ timing: { ...newTiming(), backoffMinMs: 60_000, backoffMaxMs: 60_000 } });
    await gateway.stop();
    await untilState(h, "reconnecting");
    // The wait before the next pass is long, and nothing is tried during it.
    const tries = phone.sockets.connections.length;
    await gateway.start();
    await sleep(80);
    expect(h.client.state).toBe("reconnecting");
    expect(phone.sockets.connections.length).toBe(tries);
    const started = Date.now();
    h.client.wake();
    await untilState(h, "online", 2_000);
    expect(Date.now() - started).toBeLessThan(1_000);

    // The same from unreachable: a pass fails, the long wait begins, wake ends it.
    await gateway.stop();
    await untilState(h, "reconnecting");
    h.client.wake();
    await untilState(h, "unreachable");
    await gateway.start();
    await sleep(80);
    expect(h.client.state).toBe("unreachable");
    h.client.wake();
    await untilState(h, "online", 2_000);
    expect(h.states).toEqual(["connecting", "online", "reconnecting", "online", "reconnecting", "unreachable", "online"]);
  });

  it("wake on a session that died without a sign replaces it", async () => {
    let silent = false;
    const inner = phone.sockets.factory;
    const h = await paired({
      socketFactory: (url, handlers) => inner(url, { ...handlers, onMessage: (data) => (silent ? undefined : handlers.onMessage(data)), onClose: (info) => (silent ? undefined : handlers.onClose(info)) }),
    });
    // Alive: the probe is answered and nothing changes.
    h.client.wake();
    await sleep(250);
    expect(h.states).toEqual(["connecting", "online"]);
    // Dead without a sign: nothing arrives any more, not even the close.
    silent = true;
    h.client.wake();
    await untilEntered(h, "reconnecting");
    silent = false;
    await untilEntered(h, "online", 2);
    expect(h.pairCalls).toBe(1);
  });

  it("a newly discovered address is tried at once", async () => {
    const h = await paired({ timing: { ...newTiming(), backoffMinMs: 60_000, backoffMaxMs: 60_000 } });
    await gateway.stop();
    await untilState(h, "reconnecting");
    h.client.wake();
    await untilState(h, "unreachable");
    await gateway.start(0);
    await sleep(50);
    expect(h.client.state).toBe("unreachable");
    // The same addresses again change nothing; a new one is tried at once.
    h.client.setDiscovered([{ host: "127.0.0.1", port: gateway.port }]);
    await untilState(h, "online", 2_000);
    expect(phone.store.values.get("overseer.lastAddress")).toBe(JSON.stringify({ host: "127.0.0.1", port: gateway.port }));
  });

  it("stop ends the retries; start resumes them", async () => {
    const h = await paired();
    await h.client.stop();
    expect(h.client.state).toBe("reconnecting");
    await waitFor(() => gateway.sessions === 0, "the session to end");
    const tries = phone.sockets.connections.length;
    await sleep(60);
    expect(phone.sockets.connections.length).toBe(tries);
    await h.client.start();
    await untilState(h, "online");
    expect(h.pairCalls).toBe(1);
  });
});

describe("pairing through the client", () => {
  it("fails with what happened at each address, and leaves the app unpaired", async () => {
    const h = client({ extras: [{ host: "127.0.0.1" }] });
    await h.client.start();
    const code = gateway.openPairing({ addresses: ["127.0.0.9"], confirm: () => false });
    const error = await h.client.pair(code, "Test Phone", "ios").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(PairingError);
    expect((error as PairingError).attempts.map((a) => `${a.address.host} ${a.kind} ${a.outcome}`)).toEqual(["127.0.0.9 pairing unreachable", "127.0.0.1 pairing refused"]);
    expect(h.client.state).toBe("unpaired");
    expect(phone.secrets.keys()).toEqual([]);
    expect(phone.store.keys()).toEqual(["overseer.counter"]);
  });

  it("with a wrong secret the phone is refused and the owner is never asked", async () => {
    const h = client();
    await h.client.start();
    const genuine = decodePairingCode(gateway.openPairing());
    const mistyped = encodePairingCode({ ...genuine, secret: genuine.secret.map((b, i) => (i === 3 ? b ^ 0x10 : b)) });
    const error = await h.client.pair(mistyped, "Test Phone", "ios").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(PairingError);
    expect((error as PairingError).attempts.map((a) => a.outcome)).toEqual(["refused"]);
    expect(gateway.confirmCalls).toBe(0);
    expect(gateway.refusals).toEqual(["a wrong pairing secret"]);
    expect(gateway.devices.size).toBe(0);
    expect(h.client.state).toBe("unpaired");
    // The genuine code still works afterwards: one failure does not close pairing.
    await h.client.pair(encodePairingCode(genuine), "Test Phone", "ios");
    await untilState(h, "online");
    expect(gateway.confirmCalls).toBe(1);
  });

  it("pairs at the addresses of the code and the platform's extras, in that order, and nowhere else", async () => {
    const h = client({ extras: [{ host: "127.0.0.1" }] });
    await h.client.start();
    h.client.setDiscovered([{ host: "127.0.0.7", port: gateway.port }]);
    await h.client.pair(gateway.openPairing({ addresses: ["127.0.0.8"] }), "Test Phone", "ios");
    expect(h.attempts.filter((a) => a.kind === "pairing").map((a) => `${a.address.host} ${a.outcome}`)).toEqual(["127.0.0.8 unreachable", "127.0.0.1 connected"]);
    await untilState(h, "online");
    expect(h.client.gateway?.gatewayName).toBe("Test Mac");
  });

  it("refuses a code that cannot be read, without trying anything", async () => {
    const h = client();
    await h.client.start();
    await expect(h.client.pair("OVSR1-NOTACODE", "Test Phone", "ios")).rejects.toMatchObject({ code: "bad_pairing_code" });
    expect(phone.sockets.connections).toEqual([]);
  });

  it("creates new device keys for every pairing and keeps them out of the ordinary store and the log", async () => {
    const h = await paired();
    const keys = JSON.parse(phone.secrets.values.get("overseer.keys") as string) as Record<string, string>;
    expect(Object.keys(keys).sort()).toEqual(["devicePrivateKey", "devicePublicKey", "gatewayPublicKey", "v"]);
    expect(keys["devicePublicKey"]).toBe(gateway.devices.get("device-1")?.publicKey);
    const ordinary = [...phone.store.values.values(), ...phone.store.writes.map((w) => w.value ?? "")].join("\n");
    const said = h.logs.join("\n");
    for (const secret of [keys["devicePrivateKey"] as string, keys["devicePublicKey"] as string]) {
      expect(ordinary).not.toContain(secret);
      expect(said).not.toContain(secret);
    }
    expect(said).not.toContain("OVSR1");

    await h.client.forget();
    const second = client();
    await second.client.start();
    await second.client.pair(gateway.openPairing(), "Test Phone", "ios");
    const next = JSON.parse(phone.secrets.values.get("overseer.keys") as string) as Record<string, string>;
    expect(next["devicePrivateKey"]).not.toBe(keys["devicePrivateKey"]);
  });

  it("does not pair twice", async () => {
    const h = await paired();
    await expect(h.client.pair(gateway.openPairing(), "Again", "ios")).rejects.toMatchObject({ code: "already_paired" });
    expect(gateway.pairingHandshakes).toBe(1);
    expect(gateway.devices.size).toBe(1);
  });

  it("treats a pairing whose keys are gone from the keystore as no pairing", async () => {
    const h = await paired();
    await h.client.stop();
    phone.secrets.values.clear();
    const second = client();
    await second.client.start();
    expect(second.client.state).toBe("unpaired");
  });
});

function newTiming() {
  return { backoffMinMs: 4, backoffMaxMs: 30, offRetryMs: 25, openTimeoutMs: 1_000, handshakeTimeoutMs: 2_000, pairingTimeoutMs: 3_000, requestTimeoutMs: 2_999, probeTimeoutMs: 200, contactSaveMs: 0 };
}
