import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { PhoneClient } from "../src/client.ts";
import type { RequestOptions } from "../src/client-types.ts";
import { RequestError } from "../src/errors.ts";
import { HandshakeCounter } from "../src/counter.ts";
import type { JsonObject, JsonValue } from "../src/json.ts";
import { hexToBytes } from "../src/bytes.ts";
import { connectSession } from "../src/session.ts";
import { isRequestId, uuidV4 } from "../src/uuid.ts";
import { type Harness, newClient, newPhone, type Phone, realRandom, seededRandom, sleep, untilEntered, untilState, waitFor } from "./helpers.ts";
import { MockGateway } from "./mock-gateway.ts";

let gateway: MockGateway;
let phone: Phone;
const running: Harness[] = [];

beforeEach(async () => {
  gateway = new MockGateway({ controlDelayMs: 15 });
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

async function paired(overrides: Parameters<typeof newClient>[1] = {}): Promise<Harness> {
  const h = client(overrides);
  await h.client.start();
  await h.client.pair(gateway.openPairing(), "Test Phone", "ios");
  await untilState(h, "online");
  return h;
}

/** A follow-up to an agent: a control method of the protocol. */
function followUp(client: PhoneClient, prompt: string, options: RequestOptions = {}): Promise<unknown> {
  return client.request("run.follow_up", { run_id: "run-1", prompt }, options);
}

function followUps(): { id: JsonValue; requestId: string | null; prompt: JsonValue }[] {
  return gateway.received.filter((r) => r.method === "run.follow_up").map((r) => ({ id: r.id, requestId: r.requestId, prompt: r.params["prompt"] ?? null }));
}

function stored(): { requestId: string; attempts: number; firstSentAt: number | null }[] {
  const text = phone.store.values.get("overseer.outbox");
  return text ? (JSON.parse(text) as { requestId: string; attempts: number; firstSentAt: number | null }[]) : [];
}

describe("sent exactly once (AC-122)", () => {
  it("gives every control request a request id: a UUID made from the injected random source", async () => {
    const h = await paired({ random: seededRandom(42, true) });
    await followUp(h.client, "one");
    await followUp(h.client, "two");
    const ids = followUps().map((r) => r.requestId as string);
    expect(ids.length).toBe(2);
    for (const id of ids) {
      expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
      expect(isRequestId(id)).toBe(true);
    }
    expect(ids[0]).not.toBe(ids[1]);
    expect(uuidV4(() => hexToBytes("000102030405060708090a0b0c0d0e0f"))).toBe("00010203-0405-4607-8809-0a0b0c0d0e0f");
  });

  it("decides from the method's class in the protocol description, not from the caller", async () => {
    const h = await paired();
    await followUp(h.client, "by class");
    await h.client.request("state", {});
    await h.client.request("events.list", { after: 0 }).catch(() => undefined);
    expect(followUps().map((r) => isRequestId(r.requestId ?? ""))).toEqual([true]);
    expect(gateway.received.filter((r) => r.method === "state" || r.method === "events.list").map((r) => r.requestId)).toEqual([null, null]);

    // By name: the class still decides for a method the description knows.
    await h.client.requestRaw("run.follow_up", { run_id: "run-1", prompt: "by name" }, { control: false });
    await h.client.requestRaw("state", {}, { control: true });
    expect(followUps().map((r) => isRequestId(r.requestId ?? ""))).toEqual([true, true]);
    expect(gateway.received.filter((r) => r.method === "state").map((r) => r.requestId)).toEqual([null, null]);
    expect(gateway.executions).toBe(2);

    // A method added to the daemon later: the caller says what it is.
    await expect(h.client.requestRaw("future.change", { x: 1 }, { control: true })).rejects.toMatchObject({ code: "unknown_method" });
    await expect(h.client.requestRaw("future.read", { x: 1 })).rejects.toMatchObject({ code: "unknown_method" });
    const future = gateway.received.filter((r) => r.method.startsWith("future."));
    expect(future.map((r) => [r.method, isRequestId(r.requestId ?? "")])).toEqual([["future.change", true], ["future.read", false]]);
    expect(h.client.outbox().map((e) => `${e.method} ${e.state}`)).toEqual(["run.follow_up done", "run.follow_up done", "future.change failed"]);

    await expect(h.client.requestRaw("state", "not an object")).rejects.toMatchObject({ code: "invalid_params" });
    await expect(h.client.requestRaw("state", [1, 2])).rejects.toMatchObject({ code: "invalid_params" });
    expect(await h.client.requestRaw("state")).toMatchObject({ cursor: 2 });
  });

  it("the same request id sent three times at once runs once and gets three identical replies", async () => {
    const h = await paired();
    const keys = JSON.parse(phone.secrets.values.get("overseer.keys") as string) as { devicePrivateKey: string; gatewayPublicKey: string };
    const session = await connectSession({
      url: `ws://127.0.0.1:${gateway.port}/v1`,
      kind: "session",
      socketFactory: phone.sockets.factory,
      staticPrivateKey: hexToBytes(keys.devicePrivateKey),
      gatewayPublicKey: hexToBytes(keys.gatewayPublicKey),
      hello: { device: h.client.gateway?.deviceId as string, name: "Test Phone", platform: "ios", app: "0.1.0" },
      counter: new HandshakeCounter(phone.store, "overseer.counter", Date.now),
      random: realRandom,
      now: Date.now,
    }).result;
    const replies: JsonObject[] = [];
    session.listen({ onMessage: (value) => void replies.push(value as JsonObject), onClose: () => undefined });
    const requestId = uuidV4(realRandom);
    for (const id of [11, 12, 13]) session.send({ id, method: "run.follow_up", params: { prompt: "three at once" }, request_id: requestId });
    await waitFor(() => replies.length === 3, "three replies");
    session.close();

    expect(gateway.executions).toBe(1);
    expect(replies.map((r) => r["id"]).sort()).toEqual([11, 12, 13]);
    expect(replies.map((r) => JSON.stringify(r["result"]))).toEqual(Array(3).fill(JSON.stringify({ turn: 1, prompt: "three at once" })));
    expect(gateway.events.filter((e) => (e.payload as JsonObject)["turn"] === 1).length).toBe(1);
  });

  it("the same action retried three times at once through the client runs once, with identical replies", async () => {
    const h = await paired();
    const requestId = uuidV4(realRandom);
    const send = () => followUp(h.client, "retried", { requestId });
    const results = await Promise.all([send(), send(), send()]);
    expect(results).toEqual(Array(3).fill({ turn: 1, prompt: "retried" }));
    expect(gateway.executions).toBe(1);
    // A retry after the answer gets the same answer, and nothing runs.
    expect(await send()).toEqual({ turn: 1, prompt: "retried" });
    expect(gateway.executions).toBe(1);
    expect(new Set(followUps().map((r) => r.requestId))).toEqual(new Set([requestId]));
  });

  it("a request id given to another action is refused by the client", async () => {
    const h = await paired();
    const requestId = uuidV4(realRandom);
    await followUp(h.client, "first", { requestId });
    await expect(followUp(h.client, "second", { requestId })).rejects.toMatchObject({ code: "request_id_conflict" });
    await expect(followUp(h.client, "x", { requestId: "short" })).rejects.toMatchObject({ code: "bad_request_id" });
    expect(gateway.executions).toBe(1);
  });

  it("cut after the request ran and before the reply, the retry is automatic and it ran once", async () => {
    const h = await paired();
    gateway.cutBeforeReply(1);
    const result = await followUp(h.client, "cut before the reply");
    expect(result).toEqual({ turn: 1, prompt: "cut before the reply" });
    expect(gateway.executions).toBe(1);

    const sent = followUps();
    expect(sent.length).toBe(2);
    expect(sent[0]?.requestId).toBe(sent[1]?.requestId);
    expect(sent[0]?.id).not.toBe(sent[1]?.id);
    expect(h.states.slice(-3)).toEqual(["online", "reconnecting", "online"]);
    expect(h.outbox.map((e) => `${e.state} ${e.attempts}`)).toEqual(["queued 0", "sending 1", "queued 1", "sending 2", "done 2"]);
    await waitFor(() => stored().length === 0, "the answered request to leave the store");
  });

  it("cut three times in a row before the reply, it still ran once", async () => {
    const h = await paired();
    gateway.cutBeforeReply(3);
    expect(await followUp(h.client, "stubborn")).toEqual({ turn: 1, prompt: "stubborn" });
    expect(gateway.executions).toBe(1);
    expect(followUps().length).toBe(4);
    expect(new Set(followUps().map((r) => r.requestId)).size).toBe(1);
  });

  it("a request made without a connection is kept, shown as queued, and sent once when the session is back", async () => {
    const h = await paired();
    await gateway.stop();
    await untilState(h, "unreachable");

    const pending = followUp(h.client, "typed offline");
    await waitFor(() => stored().length === 1, "the request in the store");
    expect(h.client.outbox().map((e) => [e.method, e.params, e.state, e.attempts, e.firstSentAt])).toEqual([["run.follow_up", { run_id: "run-1", prompt: "typed offline" }, "queued", 0, null]]);
    expect(followUps()).toEqual([]);
    // A request that only reads is not kept: it fails at once.
    await expect(h.client.request("state", {})).rejects.toMatchObject({ code: "not_connected" });

    await gateway.start();
    expect(await pending).toEqual({ turn: 1, prompt: "typed offline" });
    expect(gateway.executions).toBe(1);
    expect(followUps().length).toBe(1);
    expect(h.client.outbox().map((e) => e.state)).toEqual(["done"]);
    await waitFor(() => stored().length === 0, "the answered request to leave the store");
  });

  it("is in the store before it is on the wire", async () => {
    const h = await paired();
    const order: string[] = [];
    const set = phone.store.set.bind(phone.store);
    phone.store.set = async (key, value) => {
      await set(key, value);
      if (key === "overseer.outbox") order.push(`stored ${(JSON.parse(value) as { attempts: number }[]).map((e) => e.attempts).join(",")}`);
    };
    phone.store.delayMs = 5;
    const before = phone.sockets.connections.at(-1)?.sent.length as number;
    const watch = setInterval(() => {
      if ((phone.sockets.connections.at(-1)?.sent.length as number) > before && !order.includes("sent")) order.push("sent");
    }, 1);
    await followUp(h.client, "stored first");
    clearInterval(watch);
    expect(order.slice(0, 3)).toEqual(["stored 0", "stored 1", "sent"]);
  });

  it("queued requests are sent in the order they were made", async () => {
    const h = await paired();
    await gateway.stop();
    await untilState(h, "unreachable");
    const prompts = ["first", "second", "third", "fourth", "fifth"];
    const pending = prompts.map((prompt) => followUp(h.client, prompt));
    await waitFor(() => stored().length === 5, "five stored requests");
    await gateway.start();
    const results = (await Promise.all(pending)) as JsonObject[];
    expect(followUps().map((r) => r.prompt)).toEqual(prompts);
    expect(results.map((r) => r["prompt"])).toEqual(prompts);
    expect(results.map((r) => r["turn"])).toEqual([1, 2, 3, 4, 5]);
    expect(gateway.executions).toBe(5);
  });

  it("the outbox survives a restart of the app and is sent once, in order", async () => {
    const h = await paired();
    await gateway.stop();
    await untilState(h, "unreachable");
    void followUp(h.client, "before the restart 1").catch(() => undefined);
    void followUp(h.client, "before the restart 2").catch(() => undefined);
    await waitFor(() => stored().length === 2, "two stored requests");
    const ids = stored().map((e) => e.requestId);
    await h.client.stop();

    // The app is opened again: a new client over the same stores.
    const second = client();
    await second.client.start();
    expect(second.client.outbox().map((e) => [e.requestId, e.state, e.params["prompt"]])).toEqual([
      [ids[0], "queued", "before the restart 1"],
      [ids[1], "queued", "before the restart 2"],
    ]);
    await gateway.start();
    await waitFor(() => second.client.outbox().every((e) => e.state === "done"), "both answered");
    expect(followUps().map((r) => [r.requestId, r.prompt])).toEqual([
      [ids[0], "before the restart 1"],
      [ids[1], "before the restart 2"],
    ]);
    expect(gateway.executions).toBe(2);
    expect(second.client.outbox().map((e) => e.result)).toEqual([
      { turn: 1, prompt: "before the restart 1" },
      { turn: 2, prompt: "before the restart 2" },
    ]);
    await waitFor(() => stored().length === 0, "the answered requests to leave the store");
    expect(second.pairCalls).toBe(0);
  });

  it("a request that ran before the app was killed is not run again after the restart", async () => {
    const h = await paired();
    gateway.cutBeforeReply(1);
    void followUp(h.client, "ran, then the app died").catch(() => undefined);
    await waitFor(() => gateway.executions === 1, "the request to run");
    // Killed before the retry: nothing more is written, the client is gone.
    phone.store.frozen = true;
    await h.client.stop();
    phone.store.frozen = false;
    expect(stored().map((e) => e.attempts)).toEqual([1]);

    const second = client();
    await second.client.start();
    await waitFor(() => second.client.outbox()[0]?.state === "done", "the stored outcome");
    expect(second.client.outbox()[0]?.result).toEqual({ turn: 1, prompt: "ran, then the app died" });
    expect(gateway.executions).toBe(1);
  });

  it("removes an entry when the reply is an error, and tells the caller", async () => {
    const h = await paired();
    const failed = h.client.requestRaw("run.follow_up", { wrong: true });
    await expect(failed).rejects.toBeInstanceOf(RequestError);
    await expect(failed).rejects.toMatchObject({ code: "failed", message: "missing string parameter prompt" });
    expect(h.client.outbox().map((e) => [e.state, e.error?.code])).toEqual([["failed", "failed"]]);
    await waitFor(() => stored().length === 0, "the answered request to leave the store");
    gateway.dropAll();
    await untilEntered(h, "reconnecting");
    await untilEntered(h, "online", 2);
    await waitFor(() => gateway.liveSubscribers === 1, "the new session to settle");
    expect(followUps().length).toBe(1);
    h.client.dismiss(h.client.outbox()[0]?.requestId as string);
    expect(h.client.outbox()).toEqual([]);
  });

  it("a timeout ends the waiting, not the request", async () => {
    const h = await paired();
    await gateway.stop();
    await untilState(h, "unreachable");
    await expect(followUp(h.client, "slow", { timeoutMs: 30 })).rejects.toMatchObject({ code: "request_timeout" });
    expect(h.client.outbox().map((e) => e.state)).toEqual(["queued"]);
    await gateway.start();
    await waitFor(() => h.client.outbox()[0]?.state === "done", "the answer");
    expect(gateway.executions).toBe(1);
  });

  it("does not send again what was sent more than a day ago and never answered", async () => {
    const clock = { now: Date.now() };
    const h = await paired({ now: () => clock.now });
    gateway.cutBeforeReply(1);
    await gateway.stop();
    await untilState(h, "unreachable");
    await gateway.start();
    await untilState(h, "online");
    const lost = followUp(h.client, "a day old");
    await waitFor(() => gateway.executions === 1, "the request to run");
    await gateway.stop();
    await untilState(h, "unreachable");
    const fresh = followUp(h.client, "never sent");

    clock.now += 25 * 60 * 60 * 1000;
    await gateway.start();
    await expect(lost).rejects.toMatchObject({ code: "outcome_unknown" });
    // What was never sent has no such limit.
    expect(await fresh).toEqual({ turn: 2, prompt: "never sent" });
    expect(followUps().map((r) => r.prompt)).toEqual(["a day old", "never sent"]);
    expect(gateway.executions).toBe(2);
  });

  it("outcome_unknown is a final failure: it is shown and never sent again", async () => {
    const h = await paired();
    gateway.stopWhileNextRuns(1);
    const error = await followUp(h.client, "the Mac stopped while this ran").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(RequestError);
    expect(error).toMatchObject({ code: "outcome_unknown", message: expect.stringContaining("not run again") as unknown });
    expect((error as RequestError).requestId).toBe(followUps()[0]?.requestId);

    // Sent once, cut, retried once with the same id, answered: and that was the end of it.
    expect(followUps().length).toBe(2);
    expect(new Set(followUps().map((r) => r.requestId)).size).toBe(1);
    expect([gateway.interrupted, gateway.executions]).toEqual([1, 0]);
    expect(h.client.outbox().map((e) => [e.state, e.error?.code, e.attempts])).toEqual([["failed", "outcome_unknown", 2]]);
    await waitFor(() => stored().length === 0, "the answered request to leave the store");

    for (let round = 0; round < 3; round++) {
      gateway.dropAll();
      await untilEntered(h, "online", 3 + round);
      await waitFor(() => gateway.liveSubscribers === 1, "the new session to settle");
    }
    // Nor after a restart of the app.
    await h.client.stop();
    const second = client();
    await second.client.start();
    await untilState(second, "online");
    await waitFor(() => gateway.liveSubscribers === 1, "the new session to settle");
    expect(second.client.outbox()).toEqual([]);
    expect(followUps().length).toBe(2);
    expect(gateway.executions).toBe(0);

    // Asking again with the same id gives the same answer without running anything.
    await expect(followUp(h.client, "the Mac stopped while this ran", { requestId: followUps()[0]?.requestId as string })).rejects.toMatchObject({ code: "outcome_unknown" });
    expect(followUps().length).toBe(2);
  });

  it("passes the code, the message and the data of an error reply through", async () => {
    const h = await paired();
    gateway.answerElsewhere("permission-7", true, "vscode");
    const error = await h.client.request("run.permission", { run_id: "run-1", request_id: "permission-7", allow: false }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(RequestError);
    const refused = error as RequestError;
    expect(refused.code).toBe("already_answered");
    expect(refused.message).toBe("this request was answered already");
    expect(refused.data).toEqual({ allow: true, by: "vscode", ts: expect.any(Number) as unknown });
    expect(isRequestId(refused.requestId ?? "")).toBe(true);
    // The permission's own id travels inside the parameters; the request id beside them.
    const sent = gateway.received.find((r) => r.method === "run.permission");
    expect(sent?.params).toEqual({ run_id: "run-1", request_id: "permission-7", allow: false });
    expect(sent?.requestId).toBe(refused.requestId);
    expect(sent?.requestId).not.toBe("permission-7");
    // The outbox shows the same, and it is not sent again.
    expect(h.client.outbox().map((e) => [e.state, e.error])).toEqual([["failed", { code: "already_answered", message: "this request was answered already", data: refused.data }]]);
    gateway.dropAll();
    await untilEntered(h, "online", 2);
    await waitFor(() => gateway.liveSubscribers === 1, "the new session to settle");
    expect(gateway.received.filter((r) => r.method === "run.permission").length).toBe(1);

    // The first answer itself succeeds, and an error without data has none.
    expect(await h.client.request("run.permission", { run_id: "run-1", request_id: "permission-8", allow: true })).toEqual({ ok: true });
    const plain = await h.client.request("events.list", { after: 0 }).catch((e: unknown) => e);
    expect(plain).toMatchObject({ code: "unknown_method" });
    expect((plain as RequestError).data).toBeUndefined();
    expect("data" in (h.client.outbox()[0]?.error ?? {})).toBe(true);
  });

  it("refuses a request over 1 MiB before it reaches the outbox", async () => {
    const h = await paired();
    await expect(followUp(h.client, "x".repeat(1024 * 1024))).rejects.toMatchObject({ code: "request_too_large" });
    expect(h.client.outbox()).toEqual([]);
    expect(h.client.state).toBe("online");
  });

  it("sends nothing that could not be stored", async () => {
    const h = await paired();
    const set = phone.store.set.bind(phone.store);
    phone.store.set = (key, value) => (key === "overseer.outbox" ? Promise.reject(new Error("the disk is full")) : set(key, value));
    await expect(followUp(h.client, "not stored")).rejects.toMatchObject({ code: "storage" });
    await sleep(30);
    expect(followUps()).toEqual([]);
    expect(gateway.executions).toBe(0);
  });

  it("fails requests that only read when the connection is lost, and keeps the control ones", async () => {
    const h = await paired();
    gateway.cutBeforeReply(1);
    const control = followUp(h.client, "kept");
    const read = h.client.request("state", {});
    const outcome = await Promise.allSettled([read, control]);
    expect(outcome[1]).toMatchObject({ status: "fulfilled", value: { turn: 1, prompt: "kept" } });
    // The read was answered before the cut or lost with it; it is never sent twice.
    expect(gateway.received.filter((r) => r.method === "state").length).toBe(1);
    if (outcome[0].status === "rejected") expect(outcome[0].reason).toMatchObject({ code: "connection_lost" });
  });
});
