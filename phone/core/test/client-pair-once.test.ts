import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { type Harness, newClient, newPhone, type Phone, untilEntered, untilState, waitFor } from "./helpers.ts";
import { MockGateway } from "./mock-gateway.ts";

let gateway: MockGateway;
let phone: Phone;
const running: Harness[] = [];

beforeEach(async () => {
  gateway = new MockGateway();
  await gateway.start(0);
  phone = newPhone();
});

afterEach(async () => {
  for (const h of running.splice(0)) await h.client.stop();
  await gateway.stop();
});

function client(overrides: Parameters<typeof newClient>[1] = {}): Harness {
  const h = newClient(phone, overrides);
  running.push(h);
  return h;
}

/** The pairing path, seen from three sides: the app's calls, the wire, and the gateway. */
function pairingPath(): { calls: number; onTheWire: number; atTheGateway: number; ownerAsked: number } {
  return {
    calls: running.reduce((sum, h) => sum + h.pairCalls, 0),
    onTheWire: phone.sockets.handshakes(2),
    atTheGateway: gateway.pairingHandshakes,
    ownerAsked: gateway.confirmCalls,
  };
}

const ONCE = { calls: 1, onTheWire: 1, atTheGateway: 1, ownerAsked: 1 };

describe("pair once (AC-141)", () => {
  it("after pairing, nothing ever pairs again: 20 cycles, a changed address, a gateway restart, app restarts", async () => {
    let h = client();
    await h.client.start();
    await h.client.pair(gateway.openPairing(), "Test Phone", "ios");
    await untilState(h, "online");
    expect(pairingPath()).toEqual(ONCE);
    let events = 0;
    const seqs = h.seqs;

    // 20 times connected and disconnected, from either side.
    for (let cycle = 1; cycle <= 20; cycle++) {
      if (cycle % 2 === 0) gateway.dropAll();
      else phone.sockets.cutAll();
      await untilEntered(h, "online", cycle + 1);
      gateway.emit({ cycle });
      events += 1;
      await waitFor(() => seqs.length === events, `the event of cycle ${cycle}`);
    }
    expect(h.states.filter((s) => s === "online").length).toBe(21);
    expect(gateway.accepted).toBe(1 + 21);
    expect(pairingPath()).toEqual(ONCE);

    // The Mac's address changes: the same gateway somewhere else.
    const before = gateway.port;
    await gateway.stop();
    await untilState(h, "unreachable");
    await gateway.start(0);
    expect(gateway.port).not.toBe(before);
    h.client.setDiscovered([{ host: "127.0.0.1", port: gateway.port }]);
    await untilState(h, "online");
    gateway.emit({ moved: true });
    events += 1;
    await waitFor(() => seqs.length === events, "the event after the move");
    expect(pairingPath()).toEqual(ONCE);

    // The gateway restarts: every connection is cut, then it listens again.
    const port = gateway.port;
    await gateway.stop();
    await untilState(h, "unreachable");
    await gateway.start(port);
    await untilState(h, "online");
    expect(pairingPath()).toEqual(ONCE);

    // Phone access is turned off and on again.
    await gateway.turnOff();
    await untilState(h, "off");
    await gateway.turnOn();
    await untilState(h, "online");
    expect(pairingPath()).toEqual(ONCE);

    // The app is closed and opened again, three times; once while the Mac is away.
    for (let restart = 0; restart < 3; restart++) {
      await h.client.stop();
      if (restart === 1) await gateway.stop();
      h = client();
      h.client.on("event", (event) => void seqs.push(event.seq));
      await h.client.start();
      if (restart === 1) {
        await untilState(h, "unreachable");
        await gateway.start(port);
      }
      await untilState(h, "online");
      expect(h.client.gateway?.deviceId).toBe("device-1");
      gateway.emit({ restart });
      events += 1;
      await waitFor(() => seqs.length >= events, "the event after the restart");
    }

    // Thirty days without use: only the clock is different.
    await h.client.stop();
    const later = () => Date.now() + 30 * 24 * 60 * 60 * 1000;
    h = client({ now: later });
    await h.client.start();
    await untilState(h, "online");

    expect(pairingPath()).toEqual(ONCE);
    expect(gateway.devices.size).toBe(1);
    expect(phone.secrets.writes.filter((w) => w.key === "overseer.keys").length).toBe(1);
    expect(seqs.slice(0, events)).toEqual(gateway.events.map((e) => e.seq));
    expect(running.flatMap((r) => r.states)).not.toContain("unpaired");
    expect(running.flatMap((r) => r.states)).not.toContain("revoked");
  });

  it("the device's keys are its identity: the same keys reach the same gateway by another route", async () => {
    const h = client();
    await h.client.start();
    await h.client.pair(gateway.openPairing(), "Test Phone", "ios");
    await untilState(h, "online");
    const key = gateway.devices.get("device-1")?.publicKey;
    await gateway.stop();
    await gateway.start(0);
    await untilState(h, "unreachable");
    // The route is given by hand: a manual address always works.
    h.client.setExtras([{ host: "localhost", port: gateway.port }]);
    h.client.wake();
    await untilState(h, "online");
    expect(h.attempts.at(-1)).toMatchObject({ outcome: "connected", address: { host: "localhost" } });
    expect(gateway.devices.get("device-1")?.publicKey).toBe(key);
    expect(pairingPath()).toEqual(ONCE);
  });
});
