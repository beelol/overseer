import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { JsonObject } from "../src/json.ts";
import { type Harness, newClient, newPhone, type Phone, prng, sleep, untilEntered, untilState, waitFor } from "./helpers.ts";
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

function client(shared?: { seqs: number[] }, phoneToUse: Phone = phone): Harness {
  const h = newClient(phoneToUse, {}, shared);
  running.push(h);
  return h;
}

async function paired(shared?: { seqs: number[] }): Promise<Harness> {
  const h = client(shared);
  await h.client.start();
  await h.client.pair(gateway.openPairing(), "Test Phone", "ios");
  await untilState(h, "online");
  return h;
}

/** Emits numbered events in bursts until told to stop, some of them larger than one frame. */
function stream(next: () => number, until: () => boolean): Promise<number> {
  return (async () => {
    let n = 0;
    while (!until()) {
      const burst = 1 + Math.floor(next() * 10);
      for (let i = 0; i < burst; i++) {
        n += 1;
        const large = next() < 0.01;
        gateway.emit(large ? { n, pad: "x".repeat(66_000 + Math.floor(next() * 90_000)) } : { n });
      }
      await sleep(next() < 0.5 ? 1 : 0);
    }
    return n;
  })();
}

function expectSameAsLog(h: { seqs: number[] }): void {
  const log = gateway.events.map((e) => e.seq);
  expect(h.seqs.length).toBe(log.length);
  expect(h.seqs).toEqual(log);
  expect(new Set(h.seqs).size).toBe(h.seqs.length);
  for (let i = 1; i < h.seqs.length; i++) expect(h.seqs[i]).toBe((h.seqs[i - 1] as number) + 1);
}

describe("never losing the session (AC-121)", () => {
  it("cut at 100 random points during a stream, the client's sequence is the daemon's log", async () => {
    const next = prng(20260926);
    const h = await paired();
    let cuts = 0;
    const produced = stream(next, () => cuts >= 100 && gateway.events.length >= 1_500);

    while (cuts < 100) {
      await waitFor(() => gateway.sessions > 0, "a session to cut");
      const how = next();
      if (how < 0.6) {
        // In the middle of what the gateway sends: a replay, live events, the chunks of one message.
        gateway.cutAfterFrames(Math.floor(next() * 60));
        await waitFor(() => !gateway.cutArmed, "the cut");
      } else if (how < 0.8) {
        await sleep(Math.floor(next() * 6));
        gateway.dropAll();
      } else {
        // The phone's side goes away: Wi-Fi dropped.
        await sleep(Math.floor(next() * 6));
        phone.sockets.cutAll();
        await waitFor(() => gateway.sessions === 0, "the phone's cut to arrive");
      }
      cuts += 1;
    }

    const total = await produced;
    expect(total).toBeGreaterThanOrEqual(1_500);
    await waitFor(() => h.seqs.length >= gateway.events.length, "every event to arrive", 20_000);
    await untilState(h, "online");
    expectSameAsLog(h);
    expect(gateway.accepted).toBeGreaterThanOrEqual(1 + 100);
    expect(h.pairCalls).toBe(1);
    expect(h.client.cursor).toBe(gateway.events.at(-1)?.seq);
    expect(h.events.some((e) => typeof (e.payload as JsonObject)["pad"] === "string")).toBe(true);
  });

  it("across restarts of the app (a new client, the same stores) the sequence stays the log", async () => {
    const next = prng(7);
    const shared = { seqs: [] as number[] };
    let h = await paired(shared);
    let restarts = 0;
    const produced = stream(next, () => restarts >= 12 && gateway.events.length >= 800);

    while (restarts < 12) {
      await sleep(5 + Math.floor(next() * 25));
      if (next() < 0.5) gateway.cutAfterFrames(Math.floor(next() * 30));
      await sleep(Math.floor(next() * 5));
      // The app is closed and opened again.
      await h.client.stop();
      h = client(shared);
      await h.client.start();
      restarts += 1;
    }

    await produced;
    gateway.cutAfterFrames(0);
    gateway.emit({ last: true });
    await waitFor(() => shared.seqs.length >= gateway.events.length, "every event to arrive", 20_000);
    expectSameAsLog(shared);
    expect(running.reduce((sum, r) => sum + r.pairCalls, 0)).toBe(1);
    expect(gateway.pairingHandshakes).toBe(1);
  });

  it("an app that was killed resumes from what it stored: nothing is missing", async () => {
    const h = await paired();
    for (let i = 0; i < 50; i++) gateway.emit({ n: i });
    await waitFor(() => h.seqs.length === 50, "fifty events");
    await h.client.stop();
    expect(phone.store.values.get("overseer.cursor")).toBe("50");

    const second = client();
    await second.client.start();
    await untilState(second, "online");
    // Killed: from here on nothing the app writes reaches the store.
    phone.store.frozen = true;
    for (let i = 0; i < 30; i++) gateway.emit({ n: 50 + i });
    await waitFor(() => second.seqs.length === 30, "thirty more");
    await second.client.stop();
    phone.store.frozen = false;

    const third = client();
    await third.client.start();
    gateway.emit({ n: 80 });
    await waitFor(() => third.seqs.length >= 31, "the replay after the kill");
    // Everything after the stored cursor arrives again, in order, without a gap.
    expect(third.seqs).toEqual(Array.from({ length: 31 }, (_, i) => 51 + i));
  });

  it("waits for the app to apply an event before the next one and before moving the cursor", async () => {
    const h = client();
    const applied: string[] = [];
    h.client.on("event", async (event) => {
      applied.push(`start ${event.seq} at cursor ${h.client.cursor}`);
      await sleep(4);
      applied.push(`end ${event.seq}`);
    });
    await h.client.start();
    await h.client.pair(gateway.openPairing(), "Test Phone", "ios");
    await untilState(h, "online");
    for (let i = 0; i < 4; i++) gateway.emit({ n: i });
    await waitFor(() => h.client.cursor === 4, "four events applied");
    expect(applied).toEqual([1, 2, 3, 4].flatMap((n) => [`start ${n} at cursor ${n - 1}`, `end ${n}`]));
  });

  it("drops events at or below the cursor", async () => {
    const h = await paired();
    for (let i = 0; i < 20; i++) gateway.emit({ n: i });
    await waitFor(() => h.seqs.length === 20, "twenty events");
    // A second subscription from zero makes the daemon send everything again.
    await h.client.request("events.subscribe", { after: 0 });
    gateway.emit({ n: 20 });
    await waitFor(() => h.seqs.length === 21, "the next event");
    await sleep(30);
    expectSameAsLog(h);
  });

  it("resubscribes from its cursor when the daemon says resync", async () => {
    const h = await paired();
    for (let i = 0; i < 10; i++) gateway.emit({ n: i });
    await waitFor(() => h.seqs.length === 10, "ten events");
    const subscribes = () => gateway.received.filter((r) => r.method === "events.subscribe");
    expect(subscribes().map((r) => r.params["after"])).toEqual([0]);

    gateway.resync();
    // What happens while nobody is subscribed is replayed by the new subscription.
    for (let i = 0; i < 5; i++) gateway.emit({ n: 10 + i });
    await waitFor(() => h.seqs.length === 15, "the events after the resync");
    expect(subscribes().map((r) => r.params["after"])).toEqual([0, 10]);
    expect(h.states.filter((s) => s === "reconnecting")).toEqual([]);
    expectSameAsLog(h);
  });

  it("says so when history was truncated, and goes on from what the daemon still has", async () => {
    const h = await paired();
    for (let i = 0; i < 10; i++) gateway.emit({ n: i });
    await waitFor(() => h.seqs.length === 10, "ten events");
    expect(h.truncated).toEqual([]);

    await gateway.stop();
    await untilState(h, "unreachable");
    for (let i = 0; i < 40; i++) gateway.emit({ n: 10 + i });
    gateway.truncateBefore(31);
    await gateway.start();
    await untilState(h, "online");

    await waitFor(() => h.seqs.length === 30, "the events the daemon still has");
    expect(h.truncated).toEqual([10]);
    expect(h.seqs.slice(10)).toEqual(Array.from({ length: 20 }, (_, i) => 31 + i));
    // The app reloads state, as the signal asks.
    expect(await h.client.request("state", {})).toMatchObject({ cursor: 50 });
  });

  it("marks replayed events as history and later ones as news", async () => {
    for (let i = 0; i < 3; i++) gateway.emit({ n: i });
    const h = await paired();
    await waitFor(() => h.replayed.length === 1, "the first replay");
    expect(h.client.live).toBe(true);
    gateway.emit({ n: 3 });
    await waitFor(() => h.seqs.length === 4, "a live event");
    expect(h.live).toEqual([false, false, false, true]);

    await gateway.stop();
    await untilState(h, "unreachable");
    expect(h.client.live).toBe(false);
    for (let i = 0; i < 2; i++) gateway.emit({ n: 4 + i });
    await gateway.start();
    await waitFor(() => h.replayed.length === 2, "the second replay");
    gateway.emit({ n: 6 });
    await waitFor(() => h.seqs.length === 7, "a live event");
    expect(h.live).toEqual([false, false, false, true, false, false, true]);

    gateway.resync();
    gateway.emit({ n: 7 });
    await waitFor(() => h.replayed.length === 3, "the replay after the resync");
    gateway.emit({ n: 8 });
    await waitFor(() => h.seqs.length === 9, "a live event");
    expect(h.live.slice(7)).toEqual([false, true]);
    expectSameAsLog(h);
  });

  it("reports the end of every replay", async () => {
    const h = await paired();
    await waitFor(() => h.replayed.length === 1, "the first replay");
    for (let i = 0; i < 3; i++) gateway.emit({ n: i });
    await waitFor(() => h.seqs.length === 3, "three events");
    gateway.dropAll();
    for (let i = 0; i < 3; i++) gateway.emit({ n: 3 + i });
    await waitFor(() => h.replayed.length === 2, "the second replay");
    expect(h.replayed).toEqual([0, 6]);
    expectSameAsLog(h);
  });

  it("says hello as client phone, then subscribes after the cursor, after every connect", async () => {
    const h = await paired();
    for (let round = 0; round < 3; round++) {
      gateway.emit({ round });
      await waitFor(() => h.seqs.length === round + 1, "the event");
      gateway.dropAll();
      await untilEntered(h, "reconnecting", round + 1);
      await untilEntered(h, "online", round + 2);
      await waitFor(() => gateway.liveSubscribers === 1, "the subscription");
    }
    expect(h.states).toEqual(["connecting", "online", "reconnecting", "online", "reconnecting", "online", "reconnecting", "online"]);
    const sequence = gateway.received.filter((r) => r.method !== "ping").map((r) => `${r.method} ${JSON.stringify(r.params)}`);
    expect(sequence).toEqual([0, 1, 2, 3].flatMap((cursor) => ['hello {"client":"phone"}', `events.subscribe {"after":${cursor}}`]));
  });
});
