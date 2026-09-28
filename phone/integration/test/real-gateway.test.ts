/**
 * The phone's own code (`phone/core`) against the real gateway in overseerd.
 * Criteria: AC-115 (the handshake and cipher work between the Rust daemon and the app),
 * AC-116, AC-117, AC-118, AC-119, AC-121, AC-122, AC-125, AC-141.
 */

import { afterEach, describe, expect, it } from "vitest";

import { decodePairingCode, fingerprint, type OutboxEntry, RequestError } from "../../core/src/index.ts";
import { Daemon, Forwarder, makePhone, makeRepo, type Obj, pair, type Phone, sleep, until } from "./lab.ts";

const daemons: Daemon[] = [];
const phones: Phone[] = [];
/** Entries the phone still has to send. Answered ones stay listed until the app has shown them. */
const unsent = (p: Phone): OutboxEntry[] => p.client.outbox().filter(e => e.state === "queued" || e.state === "sending");
const forwarders: Forwarder[] = [];

async function daemon(mode: string, extra: Record<string, string> = {}): Promise<Daemon> {
  const d = new Daemon(mode, extra);
  daemons.push(d);
  await d.start();
  await d.enablePhoneAccess();
  return d;
}

function phone(options: Parameters<typeof makePhone>[0] = {}): Phone {
  const p = makePhone(options);
  phones.push(p);
  return p;
}

async function forwarder(target: number): Promise<Forwarder> {
  const f = new Forwarder(target);
  forwarders.push(f);
  await f.listen();
  return f;
}

afterEach(async () => {
  for (const p of phones.splice(0)) await p.client.stop().catch(() => undefined);
  for (const f of forwarders.splice(0)) await f.close();
  for (const d of daemons.splice(0)) await d.stop();
});

const online = (p: Phone): Promise<void> => until("the phone coming online", () => p.client.state === "online");

describe("pairing and sessions with the real gateway", () => {
  it("pairs once, confirmed on the Mac, and speaks the daemon's protocol", async () => {
    const d = await daemon("echo");
    const p = phone();
    expect(p.client.state).toBe("unpaired");
    const gateway = await pair(d, p, "Integration Phone");
    await online(p);
    // The Mac lists the phone with the fingerprint of the key the phone made.
    const devices = (await d.mac("gateway.devices")).devices as Obj[];
    expect(devices).toHaveLength(1);
    expect(devices[0]!.name).toBe("Integration Phone");
    expect(devices[0]!.id).toBe(gateway.deviceId);
    expect(devices[0]!.connected).toBe(true);
    expect(gateway.scope).toBe("full");
    const status = await d.mac("gateway.status");
    expect(gateway.gatewayFingerprint).toBe(status.fingerprint);
    // Typed requests, answered by the real daemon.
    const state = await p.client.request("state", {});
    expect(state.cursor).toBeGreaterThan(0);
    expect(state).toEqual(await d.mac("state"));
    const known = await p.client.request("repo.known", {});
    expect(known.repos).toEqual([]);
    const ping = await p.client.request("ping", {});
    expect(Math.abs(ping.now_ms - Date.now())).toBeLessThan(5_000);
    expect(p.client.hello?.device).toMatchObject({ name: "Integration Phone", scope: "full" });
  });

  it("refuses a wrong code, a used code and a declined phone", async () => {
    const d = await daemon("echo");
    const started = await d.mac("gateway.pair_start");
    const code = decodePairingCode(started.code as string);
    expect(fingerprint(code.gatewayPublicKey)).toBe(started.fingerprint);
    // A code with another secret: the gateway never shows it to the owner.
    const wrong = phone({ timing: { pairingTimeoutMs: 3_000 } });
    const forged = (started.code as string).slice(0, -6) + "AAAAAA";
    await expect(wrong.client.pair(forged, "Guess", "ios")).rejects.toThrow();
    expect(((await d.mac("gateway.status")).pairing as Obj).waiting).toEqual([]);
    expect((await d.mac("gateway.devices")).devices).toEqual([]);
    // Declined on the Mac.
    const declined = phone({ timing: { pairingTimeoutMs: 5_000 } });
    const attempt = declined.client.pair(started.code as string, "Declined", "ios");
    const failed = expect(attempt).rejects.toThrow();
    await until("the request", async () => (((await d.mac("gateway.status")).pairing as Obj | null)?.waiting as Obj[] | undefined)?.length === 1);
    const waiting = (((await d.mac("gateway.status")).pairing as Obj).waiting as Obj[])[0]!;
    await d.mac("gateway.pair_confirm", { request: waiting.request as string, accept: false });
    await failed;
    expect(declined.client.state).toBe("unpaired");
    // The code was used: it pairs nothing any more.
    const late = phone({ timing: { pairingTimeoutMs: 3_000 } });
    await expect(late.client.pair(started.code as string, "Late", "ios")).rejects.toThrow();
    expect((await d.mac("gateway.devices")).devices).toEqual([]);
  });

  it("pairs once: it reconnects by itself after everything that can happen", async () => {
    const d = await daemon("echo");
    let p = phone();
    const gateway = await pair(d, p, "Paired Once");
    await online(p);
    const store = p.store;
    const secrets = p.secrets;
    const reopen = async (what: string): Promise<void> => {
      await p.client.stop();
      p = phone({ store, secrets });
      expect(p.client.state, what).toBe("unpaired"); // until it has read what it stored
      await p.client.start();
      await until(`${what}: the phone coming online`, () => p.client.state === "online");
      expect(p.client.gateway?.deviceId, what).toBe(gateway.deviceId);
      expect(p.states, what).not.toContain("unpaired");
    };
    for (let n = 1; n <= 20; n++) await reopen(`reopened ${n}`);
    await d.restart();
    await reopen("after the daemon restarted");
    // While the phone is open: the daemon restarts, and phone access goes off and on.
    await d.restart();
    await until("the phone online again", () => p.client.state === "online");
    await d.mac("gateway.disable");
    await until("the phone saying off", () => p.client.state === "off");
    expect(p.states).not.toContain("unpaired");
    await d.mac("gateway.enable", { port: d.port });
    await until("the phone online again", () => p.client.state === "online");
    // The Mac opened pairing once, and knows one device.
    const events = await d.events();
    expect(events.filter(e => e.kind === "pairing_opened")).toHaveLength(1);
    expect((await d.mac("gateway.devices")).devices).toHaveLength(1);
    // It ends only when the Mac removes the phone; then the phone offers pairing again.
    await d.mac("gateway.device_revoke", { id: gateway.deviceId as string });
    await until("the phone forgetting the Mac", () => p.client.state === "unpaired");
    expect(p.states).toContain("revoked");
    expect(await secrets.get("overseer.pairing")).toBeNull();
    const again = await pair(d, p, "Paired Again");
    expect(again.deviceId).not.toBe(gateway.deviceId);
    await online(p);
  });

  it("a phone removed on the Mac while it was away learns it when it comes back", async () => {
    const d = await daemon("echo");
    const p = phone();
    const gateway = await pair(d, p, "Away Phone");
    await online(p);
    await p.client.stop();
    await d.mac("gateway.device_revoke", { id: gateway.deviceId as string });
    const back = phone({ store: p.store, secrets: p.secrets });
    await back.client.start();
    await until("the phone learning it was removed", () => back.client.state === "unpaired" && back.states.includes("revoked"));
    expect(await back.secrets.get("overseer.keys")).toBeNull();
    expect(((await d.mac("gateway.status")).sessions as number)).toBe(0);
    // A stranger with another key learns nothing: no reply at all.
    const stranger = phone({ store: p.store, secrets: p.secrets, timing: { pairingTimeoutMs: 2_000 } });
    await stranger.client.start();
    expect(stranger.client.state).toBe("unpaired");
  });

  it("finds its Mac by key: an impostor is refused and the real Mac answers", async () => {
    const real = await daemon("echo");
    const impostor = await daemon("echo");
    const p = phone();
    await pair(real, p, "Finding Phone");
    await online(p);
    await p.client.stop();
    // The impostor's address is tried first.
    const again = phone({ store: p.store, secrets: p.secrets });
    const attempts: string[] = [];
    again.client.on("attempt", a => void attempts.push(`${a.address.port}:${a.outcome}`));
    await again.store.delete("overseer.lastAddress");
    again.client.setDiscovered([{ host: "127.0.0.1", port: impostor.port }]);
    await again.client.start();
    await online(again);
    expect(attempts.some(a => a.startsWith(`${impostor.port}:`) && !a.endsWith("connected"))).toBe(true);
    expect(attempts).toContain(`${real.port}:connected`);
    expect(((await impostor.mac("gateway.status")).sessions as number)).toBe(0);
  });
});

describe("never losing the session", () => {
  it("a stream cut at a hundred random points arrives once and in order", async () => {
    const d = await daemon("echo");
    const f = await forwarder(d.port);
    const p = phone();
    await pair(d, p, "Resuming Phone", f.port);
    await online(p);
    await p.client.stop();
    // From here on the phone reaches the Mac only through the forwarder.
    const through = phone({ store: p.store, secrets: p.secrets });
    const repo = makeRepo();
    const start = (await d.mac("state")).cursor as number;
    const created = await d.mac("task.create", { repo, harness: "generic", workspace_mode: "worktree", program: "/bin/sh", prompt: "", title: "numbered lines",
      args: ["-c", "i=0; while [ $i -lt 3000 ]; do echo line$i; i=$((i+1)); if [ $((i % 25)) -eq 0 ]; then sleep 0.04; fi; done"] });
    const run = (created.run as Obj).id as string;
    // Only what came after the start is compared.
    await through.store.set("overseer.cursor", String(start));
    await through.client.start();
    let seed = 20260926;
    const roll = (max: number): number => {
      seed = (Math.imul(seed, 1103515245) + 12345) >>> 0;
      return seed % max;
    };
    let cuts = 0;
    let liveCuts = 0;
    while (cuts < 100) {
      const before = through.events.length;
      const take = 1 + roll(60);
      const end = Date.now() + 250;
      while (through.events.length < before + take && Date.now() < end) await sleep(5);
      if ((await d.run(run)).status === "running") liveCuts += 1;
      if (cuts === 50) await d.restart(); // the daemon is killed mid-stream
      f.cut();
      cuts += 1;
    }
    expect(f.accepted).toBeGreaterThan(80);
    await d.waitDone(run, 60_000);
    const expected = (await d.events(start)) as unknown as typeof through.events;
    await until("the rest of the stream", () => through.events.length >= expected.length, 30_000);
    await sleep(300);
    expect(through.events.map(e => e.seq)).toEqual(expected.map(e => e.seq));
    expect(through.events).toEqual(expected);
    const lines = through.events.filter(e => e.kind === "output" && e.run_id === run).map(e => (e.payload as Obj).text);
    expect(lines).toHaveLength(3000);
    expect(lines.every((text, n) => text === `line${n}`)).toBe(true);
    expect(through.client.cursor).toBe(expected[expected.length - 1]!.seq);
    expect(liveCuts).toBeGreaterThan(30);
    expect(through.states).not.toContain("unpaired");
    console.log(`resume: 100 cuts (${liveCuts} while the stream was live), ${f.accepted} connections, ${expected.length} events, identical`);
  });

  it("says so when history is gone, and when the Mac cannot be reached", async () => {
    const d = await daemon("echo");
    const f = await forwarder(d.port);
    const p = phone();
    await pair(d, p, "Away Phone", f.port);
    await online(p);
    await p.client.stop();
    const away = phone({ store: p.store, secrets: p.secrets });
    // While the phone is away, an agent writes more than the daemon keeps.
    const repo = makeRepo();
    const created = await d.mac("task.create", { repo, harness: "generic", workspace_mode: "worktree", program: "/bin/sh", prompt: "", title: "a long run",
      args: ["-c", "i=0; while [ $i -lt 12000 ]; do echo x$i; i=$((i+1)); done"] });
    await d.waitDone((created.run as Obj).id as string, 60_000);
    await away.client.start();
    await until("the phone noticing", () => away.truncated > 0);
    expect(away.truncated).toBe(1);
    // The Mac goes away: the phone says unreachable, keeps the time of the last contact, and keeps trying.
    const contact = away.client.lastContact;
    expect(contact).not.toBeNull();
    f.down = true;
    f.cut();
    d.kill();
    await until("the phone saying the Mac is unreachable", () => away.client.state === "unreachable", 20_000);
    expect(away.client.lastContact).toBeGreaterThanOrEqual(contact!);
    expect(Date.now() - away.client.lastContact!).toBeLessThan(30_000);
    await d.start();
    f.down = false;
    await until("the phone online again", () => away.client.state === "online", 20_000);
  });
});

describe("sent exactly once", () => {
  it("a message sent while the connection comes and goes makes one turn", async () => {
    const d = await daemon("echo");
    const f = await forwarder(d.port);
    const p = phone();
    await pair(d, p, "Sending Phone", f.port);
    await online(p);
    await p.client.stop();
    const sender = phone({ store: p.store, secrets: p.secrets });
    await sender.client.start();
    await online(sender);
    const repo = makeRepo();
    const first = await d.mac("task.create", { repo, harness: "claude", prompt: "first", title: "exactly once" });
    const run = (first.run as Obj).id as string;
    await d.waitDone(run);
    const turns = async (): Promise<string[]> => ((await d.mac<Obj[]>("run.turns", { run_id: run })) as Obj[]).map(t => t.prompt as string);

    // Typed without a connection: queued, shown as queued, sent once when the Mac is back.
    f.down = true;
    f.cut();
    await until("the phone noticing", () => sender.client.state !== "online");
    const seen: OutboxEntry[] = [];
    sender.client.on("outbox", entry => void seen.push({ ...entry }));
    const queued = sender.client.request("run.follow_up", { run_id: run, prompt: "typed in a tunnel" }, { timeoutMs: 60_000 });
    await until("the message in the outbox", () => unsent(sender).length === 1);
    expect(unsent(sender)[0]!.state).toBe("queued");
    expect(seen.map(e => e.state)).toEqual(["queued"]);
    expect(await turns()).toEqual(["first"]);
    // The app is closed and opened again: the message is still waiting.
    await sender.client.stop();
    void queued.catch(() => undefined);
    const reopened = phone({ store: sender.store, secrets: sender.secrets });
    await reopened.client.start();
    expect(unsent(reopened).map(e => (e.params as Obj).prompt)).toEqual(["typed in a tunnel"]);
    f.down = false;
    await until("the message being sent", () => unsent(reopened).length === 0, 30_000);
    expect(reopened.client.outbox().map(e => e.state)).toEqual(["done"]);
    await d.waitDone(run);
    expect(await turns()).toEqual(["first", "typed in a tunnel"]);

    // The connection is cut again and again while messages are sent: each makes one turn.
    for (const prompt of ["second", "third", "fourth"]) {
      const sending = reopened.client.request("run.follow_up", { run_id: run, prompt }, { timeoutMs: 60_000 });
      await sleep(3);
      f.cut();
      await sleep(30);
      f.cut();
      const turn = await sending;
      expect(turn.prompt).toBe(prompt);
      await d.waitDone(run);
    }
    expect(await turns()).toEqual(["first", "typed in a tunnel", "second", "third", "fourth"]);
    // Every one of them is the phone's, by name.
    const commands = (await d.events()).filter(e => e.kind === "remote_command");
    expect(commands).toHaveLength(4);
    expect(commands.every(e => e.source === "phone:Sending Phone")).toBe(true);
    // An error from the daemon is the answer, not a reason to try again.
    await expect(reopened.client.request("run.follow_up", { run_id: "r-missing", prompt: "x" })).rejects.toBeInstanceOf(RequestError);
    expect(unsent(reopened)).toEqual([]);
  });
});

describe("what a phone may do", () => {
  it("answers a permission request once, and learns when the Mac was first", async () => {
    const d = await daemon("permission");
    const p = phone();
    await pair(d, p, "Answering Phone");
    await online(p);
    const repo = makeRepo();
    const ask = async (): Promise<{ run: string; request: string }> => {
      const created = await d.mac("task.create", { repo, harness: "claude", prompt: "write perm.txt", title: "permission" });
      const run = (created.run as Obj).id as string;
      const waiting = await d.waitStatus(run, "waiting_for_user");
      return { run, request: (waiting.attention as Obj).request_id as string };
    };
    // The phone allows.
    const one = await ask();
    await until("the request on the phone", () => p.events.some(e => e.kind === "permission" && e.run_id === one.run));
    expect(await p.client.request("run.permission", { run_id: one.run, request_id: one.request, allow: true })).toEqual({ ok: true });
    expect((await d.waitDone(one.run)).status).toBe("completed");
    const answered = (await d.events()).find(e => e.kind === "permission_answered" && e.run_id === one.run)!;
    expect(answered.source).toBe("phone:Answering Phone");
    // The Mac was first: the phone is told what the answer was, and by whom.
    const two = await ask();
    await d.mac("run.permission", { run_id: two.run, request_id: two.request, allow: false });
    const late = p.client.request("run.permission", { run_id: two.run, request_id: two.request, allow: true });
    await expect(late).rejects.toMatchObject({ code: "already_answered", data: { allow: false, by: "the Mac" } });
    await d.waitDone(two.run);
    // The phone saw the Mac's answer in its own stream.
    await until("the Mac's answer on the phone", () => p.events.some(e => e.kind === "permission_answered" && e.run_id === two.run));
  });

  it("a watch-only phone reads and changes nothing; Mac-only methods stay on the Mac", async () => {
    const d = await daemon("slow", { FIXTURE_SLOW_MS: "20000" });
    const p = phone();
    const gateway = await pair(d, p, "Watching Phone");
    await online(p);
    const repo = makeRepo();
    const created = await d.mac("task.create", { repo, harness: "claude", prompt: "long", title: "long" });
    const run = (created.run as Obj).id as string;
    await d.waitStatus(run, "running");
    await d.mac("gateway.device_scope", { id: gateway.deviceId as string, scope: "watch" });
    expect((await p.client.request("state", {})).runs.find(r => r.id === run)?.status).toBe("running");
    await expect(p.client.request("run.interrupt", { run_id: run })).rejects.toMatchObject({ code: "watch_only" });
    await expect(p.client.request("run.follow_up", { run_id: run, prompt: "x" })).rejects.toMatchObject({ code: "watch_only" });
    expect((await d.run(run)).status).toBe("running");
    for (const method of ["gateway.disable", "gateway.pair_start", "gateway.devices", "daemon.shutdown", "daemon.stop_all"]) {
      await expect(p.client.requestRaw(method, {}, { control: true }), method).rejects.toMatchObject({ code: "mac_only" });
    }
    // Full control again, at once, with no new connection.
    await d.mac("gateway.device_scope", { id: gateway.deviceId as string, scope: "full" });
    await p.client.request("run.interrupt", { run_id: run });
    expect((await d.waitDone(run)).status).toBe("interrupted");
    expect(((await d.mac("gateway.status")).enabled)).toBe(true);
  });
});
