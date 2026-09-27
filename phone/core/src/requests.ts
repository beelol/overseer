/**
 * Requests that wait for their reply. Every request has a number of its own; the reply with
 * the same number on the same session answers it.
 */

import { OverseerError, type ReplyError, RequestError } from "./errors.ts";
import { isJsonObject, type JsonObject, type JsonValue, stringField } from "./json.ts";
import type { TimerHandle, Timers } from "./platform.ts";
import type { Session } from "./session.ts";

interface Pending {
  readonly session: Session;
  readonly resolve: (value: JsonValue) => void;
  readonly reject: (error: Error) => void;
  timer: TimerHandle | null;
}

/** Reads the error object of a reply. What is missing in it becomes `failed` and an empty message. */
export function replyError(error: JsonValue): ReplyError {
  if (!isJsonObject(error)) return { code: "failed", message: "" };
  const code = stringField(error, "code") ?? "failed";
  const message = stringField(error, "message") ?? "";
  const data = error["data"];
  return data === undefined ? { code, message } : { code, message, data };
}

export class PendingRequests {
  private readonly timers: Timers;
  private readonly waiting = new Map<number, Pending>();
  // The keepalive of the session uses 0; requests start at 1.
  private nextId = 1;

  constructor(timers: Timers) {
    this.timers = timers;
  }

  /**
   * Sends one request on `session` and waits for its reply. `timeoutMs` of 0 waits as long as
   * the session lives. A control request carries its `requestId` beside the parameters.
   */
  call(session: Session, method: string, params: JsonObject, timeoutMs: number, requestId?: string): Promise<JsonValue> {
    const id = this.nextId++;
    return new Promise<JsonValue>((resolve, reject) => {
      const entry: Pending = { session, resolve, reject, timer: null };
      this.waiting.set(id, entry);
      if (timeoutMs > 0) {
        entry.timer = this.timers.set(() => {
          entry.timer = null;
          this.waiting.delete(id);
          reject(new OverseerError("request_timeout", `no reply to ${method} in time`));
        }, timeoutMs);
      }
      try {
        session.send(requestId === undefined ? { id, method, params } : { id, method, params, request_id: requestId });
      } catch (error) {
        this.drop(id, entry);
        reject(error instanceof Error ? error : new OverseerError("closed", "the session is closed"));
      }
    });
  }

  /**
   * A reply arrived on `session`. Returns the error of an error reply, so that the client can
   * act on its code; null for a result or a reply nobody waits for.
   */
  answer(session: Session, reply: JsonObject): ReplyError | null {
    const id = reply["id"];
    if (typeof id !== "number") return null;
    const entry = this.waiting.get(id);
    if (!entry || entry.session !== session) return null;
    this.drop(id, entry);
    const error = reply["error"];
    if (error === undefined || error === null) {
      entry.resolve(reply["result"] ?? null);
      return null;
    }
    const parsed = replyError(error);
    entry.reject(new RequestError(parsed));
    return parsed;
  }

  /** Fails what waits on `session`, or everything when no session is given. */
  failAll(error: Error, session?: Session): void {
    for (const [id, entry] of [...this.waiting]) {
      if (session && entry.session !== session) continue;
      this.drop(id, entry);
      entry.reject(error);
    }
  }

  private drop(id: number, entry: Pending): void {
    this.waiting.delete(id);
    if (entry.timer !== null) this.timers.clear(entry.timer);
    entry.timer = null;
  }
}
