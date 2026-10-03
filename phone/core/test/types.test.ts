/**
 * The types of the public interface. Most of this file is checked by the compiler
 * (`npm run typecheck`): a line marked `@ts-expect-error` must not compile.
 */

import { describe, expect, expectTypeOf, it } from "vitest";
import { WebSocket as WsWebSocket } from "ws";
import type { ControlMethod, PhoneMethod, State, Turn, Result, Run, QueueSnapshot, EventPayloads } from "../../protocol/protocol.generated.ts";
import {
  CONNECTION_STATES,
  type ConnectionState,
  type DaemonEvent,
  type EventInfo,
  type GatewayInfo,
  type JsonValue,
  type KeyValueStore,
  MemoryStore,
  type OutboxEntry,
  type OutboxState,
  PhoneClient,
  RequestError,
  type SecretStore,
  type SocketFactory,
  webSocketFactory,
} from "../src/index.ts";

function make(): PhoneClient {
  return new PhoneClient({ socketFactory: webSocketFactory(WebSocket), store: new MemoryStore(), secrets: new MemoryStore(), random: (n) => new Uint8Array(n).fill(1), now: () => 0, app: "0.1.0" });
}

describe("the types of the client", () => {
  it("has exactly the seven states", () => {
    expectTypeOf<ConnectionState>().toEqualTypeOf<"unpaired" | "connecting" | "online" | "reconnecting" | "off" | "unreachable" | "revoked">();
    expectTypeOf<OutboxState>().toEqualTypeOf<"queued" | "sending" | "done" | "failed">();
    expect(CONNECTION_STATES.length).toBe(7);
  });

  it("types a request by its method: parameters in, result out", async () => {
    const client = make();
    // Checked by the compiler and never called: nothing is sent for a check of types.
    const accepted = (): void => {
      expectTypeOf(client.request("state", {})).toEqualTypeOf<Promise<State>>();
      expectTypeOf(client.request("run.follow_up", { run_id: "r", prompt: "p" })).toEqualTypeOf<Promise<Turn | { delivery: "queued" }>>();
      expectTypeOf(client.request("run.permission", { run_id: "r", request_id: "q", allow: true })).toEqualTypeOf<Promise<{ ok: boolean }>>();
      expectTypeOf(client.request("events.subscribe", { after: 5 })).resolves.toHaveProperty("history_truncated");
      expectTypeOf(client.requestRaw("anything.new", { x: 1 }, { control: true })).toEqualTypeOf<Promise<JsonValue>>();
    };
    expect(typeof accepted).toBe("function");
    // What the types promise is what happens: without a pairing every request is refused.
    await expect(client.request("state", {})).rejects.toMatchObject({ code: "unpaired" });
    await expect(client.request("run.follow_up", { run_id: "r", prompt: "p" })).rejects.toMatchObject({ code: "unpaired" });
    await expect(client.requestRaw("anything.new")).rejects.toMatchObject({ code: "unpaired" });

    const refused = (): void => {
      // @ts-expect-error a method of the Mac only is not offered to the phone
      void client.request("daemon.shutdown", {});
      // @ts-expect-error a method that does not exist
      void client.request("no.such_method", {});
      // @ts-expect-error a parameter is missing
      void client.request("run.follow_up", { prompt: "no run" });
      // @ts-expect-error a parameter of the wrong type
      void client.request("run.permission", { run_id: "r", request_id: "q", allow: "yes" });
      // @ts-expect-error the caller does not decide what is a control request
      void client.request("state", {}, { control: true });
    };
    expect(typeof refused).toBe("function");
    expectTypeOf<ControlMethod>().toMatchTypeOf<PhoneMethod>();
    expectTypeOf<"run.follow_up">().toMatchTypeOf<ControlMethod>();
    expectTypeOf<"state">().not.toMatchTypeOf<ControlMethod>();
    expectTypeOf<"daemon.shutdown">().not.toMatchTypeOf<PhoneMethod>();
  });

  it("distinguishes a queued follow-up from a launched turn and types queue state", () => {
    const accepted = (reply: Result<"run.follow_up">): void => {
      if ("delivery" in reply) {
        expectTypeOf(reply.delivery).toEqualTypeOf<"queued">();
        // @ts-expect-error a queued acknowledgement has no launched turn
        void reply.started_ms;
      } else {
        expectTypeOf(reply).toEqualTypeOf<Turn>();
      }
    };
    expect(typeof accepted).toBe("function");
    expectTypeOf<Run["queue"]>().toEqualTypeOf<QueueSnapshot | null | undefined>();
    expectTypeOf<QueueSnapshot["messages"][number]["redirect"]>().toBeBoolean();
    expectTypeOf<EventPayloads["queue_changed"]["paused"]>().toEqualTypeOf<boolean | null | undefined>();
  });

  it("types what the client reports", () => {
    const client = make();
    expectTypeOf(client.state).toEqualTypeOf<ConnectionState>();
    expectTypeOf(client.lastContact).toEqualTypeOf<number | null>();
    expectTypeOf(client.gateway).toEqualTypeOf<GatewayInfo | null>();
    expectTypeOf(client.outbox()).toEqualTypeOf<readonly OutboxEntry[]>();
    expectTypeOf(client.pair).parameters.toEqualTypeOf<[code: string, deviceName: string, platform: string]>();
    client.on("event", (event, info) => {
      expectTypeOf(event).toEqualTypeOf<DaemonEvent>();
      expectTypeOf(event.seq).toBeNumber();
      expectTypeOf(info).toEqualTypeOf<EventInfo>();
    });
    client.on("state", (state, previous) => expectTypeOf([state, previous]).toEqualTypeOf<ConnectionState[]>());
    const off = client.on("truncated", (info) => expectTypeOf(info.cursor).toBeNumber());
    expectTypeOf(off).toEqualTypeOf<() => void>();
    // @ts-expect-error an event that does not exist
    client.on("nothing", () => undefined);
  });

  it("carries the code, the message and the data of an error reply", () => {
    const error = new RequestError({ code: "already_answered", message: "answered", data: { allow: true, by: "vscode", ts: 5 } }, "request-1");
    expectTypeOf(error.code).toBeString();
    expectTypeOf(error.data).toEqualTypeOf<JsonValue | undefined>();
    expectTypeOf(error.requestId).toEqualTypeOf<string | undefined>();
    expect([error.name, error.code, error.message, error.data, error.requestId]).toEqual(["RequestError", "already_answered", "answered", { allow: true, by: "vscode", ts: 5 }, "request-1"]);
    expect(error).toBeInstanceOf(Error);
    expect(new RequestError({ code: "failed", message: "" }).message).toBe("failed");
  });

  it("takes the standard WebSocket of the platform, and the secret store has the shape of the store", () => {
    expectTypeOf(webSocketFactory(WebSocket)).toEqualTypeOf<SocketFactory>();
    expectTypeOf(webSocketFactory(WsWebSocket)).toEqualTypeOf<SocketFactory>();
    expectTypeOf<SecretStore>().toEqualTypeOf<KeyValueStore>();
    // @ts-expect-error not a WebSocket
    webSocketFactory(Date);
  });
});
