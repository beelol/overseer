/**
 * Control requests, sent exactly once (AC-122).
 *
 * A control request gets a request id when it is made and keeps it on every retry. It is
 * written to the outbox before it is sent, sent again after every reconnect and restart, in
 * the order the requests were made, and leaves the outbox only when a reply arrived. A reply
 * is final whether it is a result or an error: an error such as `outcome_unknown` is shown to
 * the user and the request is never sent again.
 */

import { OverseerError, type ReplyError, RequestError } from "./errors.ts";
import type { JsonObject, JsonValue } from "./json.ts";
import { Outbox, type OutboxEntry } from "./outbox.ts";
import type { Clock, KeyValueStore, Log, TimerHandle, Timers } from "./platform.ts";
import type { Session } from "./session.ts";

interface Waiter {
  readonly resolve: (value: JsonValue) => void;
  readonly reject: (error: Error) => void;
  timer: TimerHandle | null;
}

/** What the queue needs from the client. */
export interface ControlQueueHost {
  readonly now: Clock;
  readonly timers: Timers;
  readonly log: Log | undefined;
  /** See `ClientTiming.retryWindowMs`. */
  readonly retryWindowMs: number;
  /** The session that carries requests now, or null when there is none. */
  session(): Session | null;
  /** Sends a control request on `session` and resolves or rejects with its reply. */
  send(session: Session, entry: OutboxEntry): Promise<JsonValue>;
  /** An entry was added or changed its state. */
  changed(entry: OutboxEntry): void;
}

export class ControlQueue {
  private readonly box: Outbox;
  private readonly host: ControlQueueHost;
  private readonly waiters = new Map<string, Waiter[]>();
  private pumping = false;
  private pumpAgain = false;

  constructor(store: KeyValueStore, key: string, host: ControlQueueHost) {
    this.box = new Outbox(store, key, host.log);
    this.host = host;
  }

  /** Reads the stored outbox. What was on the wire when the app stopped waits again. */
  load(): Promise<void> {
    return this.box.load();
  }

  entries(): readonly OutboxEntry[] {
    return this.box.entries();
  }

  dismiss(requestId: string): void {
    this.box.dismiss(requestId);
  }

  /** Resolves when every write to the store that was started has ended. */
  settled(): Promise<void> {
    return this.box.settled();
  }

  /**
   * Takes a control request. With a `requestId` that is known already, the caller joins the
   * request that exists: it is not added twice, and an answer that arrived is given at once.
   */
  submit(requestId: string, method: string, params: JsonObject, timeoutMs: number | undefined): Promise<JsonValue> {
    const known = this.box.find(requestId);
    if (known) {
      if (known.method !== method || JSON.stringify(known.params) !== JSON.stringify(params)) {
        return Promise.reject(new OverseerError("request_id_conflict", "this request id belongs to another action"));
      }
      if (known.state === "done") return Promise.resolve(known.result ?? null);
      if (known.state === "failed" && known.error) return Promise.reject(new RequestError(known.error, requestId));
    }
    return new Promise<JsonValue>((resolve, reject) => {
      const waiter: Waiter = { resolve, reject, timer: null };
      this.waiters.set(requestId, [...(this.waiters.get(requestId) ?? []), waiter]);
      if (timeoutMs !== undefined && timeoutMs > 0) {
        waiter.timer = this.host.timers.set(() => {
          waiter.timer = null;
          const rest = (this.waiters.get(requestId) ?? []).filter((w) => w !== waiter);
          if (rest.length > 0) this.waiters.set(requestId, rest);
          else this.waiters.delete(requestId);
          reject(new OverseerError("request_timeout", "no answer yet; the request stays in the outbox until it is answered"));
        }, timeoutMs);
      }
      if (known) {
        void this.pump();
        return;
      }
      this.host.changed(this.box.add({ requestId, method, params, createdAt: this.host.now() }));
      this.box.save().then(
        () => void this.pump(),
        // What cannot be stored is not sent: it would not survive, and could not be retried safely.
        () => this.settle(requestId, { error: { code: "storage", message: "the request could not be stored, so it was not sent" } }),
      );
    });
  }

  /** Sends what waits, in order. Every entry is stored as sent before it is sent. */
  async pump(): Promise<void> {
    if (this.pumping) {
      this.pumpAgain = true;
      return;
    }
    this.pumping = true;
    try {
      do {
        this.pumpAgain = false;
        const now = this.host.now();
        this.expire(now);
        const session = this.host.session();
        if (!session || session.closed) break;
        const sending = this.box.markSending(
          this.box.queued().map((entry) => entry.requestId),
          now,
        );
        if (sending.length === 0) break;
        try {
          await this.box.save();
        } catch {
          this.host.log?.("the outbox could not be stored; nothing was sent");
          this.sessionEnded();
          break;
        }
        if (this.host.session() !== session || session.closed) {
          // The session ended while the outbox was stored; what it held waits for the next one.
          this.sessionEnded();
          this.pumpAgain = true;
          continue;
        }
        for (const entry of sending) this.send(session, entry);
      } while (this.pumpAgain);
    } finally {
      this.pumping = false;
    }
  }

  /** The session ended: what was on the wire waits for the next one. */
  sessionEnded(): void {
    for (const entry of this.box.requeue()) this.host.changed(entry);
  }

  /** The pairing is gone: nothing that waits can be sent any more. */
  async failAll(error: ReplyError): Promise<void> {
    for (const entry of this.box.entries()) {
      if (entry.state === "queued" || entry.state === "sending") this.settle(entry.requestId, { error });
    }
    this.box.clear();
    await this.box.save();
  }

  private send(session: Session, entry: OutboxEntry): void {
    if (this.box.find(entry.requestId)?.state !== "sending") return;
    this.host.changed(entry);
    this.host.send(session, entry).then(
      (result) => this.settle(entry.requestId, { result }),
      (error: unknown) => {
        // An error reply is an answer, and it is final. Anything else is a lost connection:
        // the entry stays and is sent again.
        if (!(error instanceof RequestError)) return;
        const reply: ReplyError = error.data === undefined ? { code: error.code, message: error.message } : { code: error.code, message: error.message, data: error.data };
        this.settle(entry.requestId, { error: reply });
      },
    );
  }

  /**
   * What was sent long ago and never answered is not sent again: the gateway may have
   * forgotten its outcome, and a retry could run the action a second time.
   */
  private expire(now: number): void {
    for (const entry of this.box.queued()) {
      if (entry.firstSentAt === null || now - entry.firstSentAt <= this.host.retryWindowMs) continue;
      this.settle(entry.requestId, {
        error: { code: "outcome_unknown", message: "this was sent and never answered, and it is too old to send again safely; check the agent before sending it again" },
      });
    }
  }

  /** A request was answered: it leaves the outbox and everyone waiting for it hears. */
  private settle(requestId: string, outcome: { result: JsonValue } | { error: ReplyError }): void {
    const done = this.box.finish(requestId, outcome);
    if (!done) return;
    this.box.save().catch(() => this.host.log?.("the outbox could not be stored"));
    this.host.changed(done);
    const list = this.waiters.get(requestId) ?? [];
    this.waiters.delete(requestId);
    for (const waiter of list) {
      if (waiter.timer !== null) this.host.timers.clear(waiter.timer);
      if ("error" in outcome) waiter.reject(new RequestError(outcome.error, requestId));
      else waiter.resolve(outcome.result);
    }
  }
}
