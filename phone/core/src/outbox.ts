/**
 * The outbox: control requests that were not answered yet, kept in the store so that they
 * survive a lost connection and a restart of the app. An entry is written before it is sent,
 * sent again after every reconnect with the same request id, and removed only when a reply
 * arrived, whether a result or an error.
 */

import type { ReplyError } from "./errors.ts";
import { isJsonObject, type JsonObject, type JsonValue, numberField, stringField } from "./json.ts";
import type { KeyValueStore, Log } from "./platform.ts";

/** Where an entry is: waiting, on the wire, or answered. */
export type OutboxState = "queued" | "sending" | "done" | "failed";

/** One control request, as the app shows it. */
export interface OutboxEntry {
  /** The request id: made once per action, the same on every retry. */
  readonly requestId: string;
  readonly method: string;
  readonly params: JsonObject;
  readonly createdAt: number;
  readonly state: OutboxState;
  /** How often it was sent. */
  readonly attempts: number;
  /** When it was sent first, or null when it never was. */
  readonly firstSentAt: number | null;
  /** The result, when `state` is `done`. */
  readonly result?: JsonValue;
  /** The error, when `state` is `failed`. */
  readonly error?: ReplyError;
}

interface Stored {
  requestId: string;
  method: string;
  params: JsonObject;
  createdAt: number;
  attempts: number;
  firstSentAt: number | null;
}

/** How many answered entries are kept in memory for the app to show. */
const FINISHED_KEPT = 50;

export class Outbox {
  private readonly store: KeyValueStore;
  private readonly key: string;
  private readonly log: Log | undefined;
  private open: OutboxEntry[] = [];
  private finished: OutboxEntry[] = [];
  private writes: Promise<void> = Promise.resolve();

  constructor(store: KeyValueStore, key: string, log?: Log) {
    this.store = store;
    this.key = key;
    this.log = log;
  }

  /** Reads the stored entries. Whatever was on the wire when the app stopped is queued again. */
  async load(): Promise<void> {
    this.open = [];
    this.finished = [];
    const text = await this.store.get(this.key);
    if (text === null) return;
    try {
      const list: unknown = JSON.parse(text);
      if (!Array.isArray(list)) return;
      for (const item of list) {
        if (!isJsonObject(item)) continue;
        const requestId = stringField(item, "requestId");
        const method = stringField(item, "method");
        const params = item["params"];
        if (!requestId || !method || !isJsonObject(params)) continue;
        this.open.push({
          requestId,
          method,
          params,
          createdAt: numberField(item, "createdAt") ?? 0,
          state: "queued",
          attempts: numberField(item, "attempts") ?? 0,
          firstSentAt: numberField(item, "firstSentAt"),
        });
      }
    } catch {
      this.log?.("the stored outbox cannot be read; it starts empty");
    }
  }

  /** Every entry: the unanswered ones in the order they were made, then the answered ones. */
  entries(): readonly OutboxEntry[] {
    return [...this.open, ...this.finished];
  }

  /** The unanswered entries that wait to be sent, in order. */
  queued(): readonly OutboxEntry[] {
    return this.open.filter((entry) => entry.state === "queued");
  }

  find(requestId: string): OutboxEntry | undefined {
    return this.open.find((entry) => entry.requestId === requestId) ?? this.finished.find((entry) => entry.requestId === requestId);
  }

  /** Adds an entry at the end, in memory. `save` makes it durable. */
  add(entry: { requestId: string; method: string; params: JsonObject; createdAt: number }): OutboxEntry {
    const added: OutboxEntry = { ...entry, state: "queued", attempts: 0, firstSentAt: null };
    this.open.push(added);
    return added;
  }

  /** Marks entries as being sent now, in memory. `save` must finish before they are sent. */
  markSending(requestIds: readonly string[], at: number): OutboxEntry[] {
    const changed: OutboxEntry[] = [];
    this.open = this.open.map((entry) => {
      if (!requestIds.includes(entry.requestId) || entry.state !== "queued") return entry;
      const next: OutboxEntry = { ...entry, state: "sending", attempts: entry.attempts + 1, firstSentAt: entry.firstSentAt ?? at };
      changed.push(next);
      return next;
    });
    return changed;
  }

  /** The connection ended: what was on the wire waits again. Returns what changed. */
  requeue(): OutboxEntry[] {
    const changed: OutboxEntry[] = [];
    this.open = this.open.map((entry) => {
      if (entry.state !== "sending") return entry;
      const next: OutboxEntry = { ...entry, state: "queued" };
      changed.push(next);
      return next;
    });
    return changed;
  }

  /** A reply arrived. The entry leaves the stored outbox and stays in memory as answered. */
  finish(requestId: string, outcome: { result: JsonValue } | { error: ReplyError }): OutboxEntry | null {
    const entry = this.open.find((e) => e.requestId === requestId);
    if (!entry) return null;
    this.open = this.open.filter((e) => e.requestId !== requestId);
    const done: OutboxEntry = "error" in outcome ? { ...entry, state: "failed", error: outcome.error } : { ...entry, state: "done", result: outcome.result };
    this.finished.push(done);
    if (this.finished.length > FINISHED_KEPT) this.finished.splice(0, this.finished.length - FINISHED_KEPT);
    return done;
  }

  /** Removes an answered entry from memory, when the app has shown it. */
  dismiss(requestId: string): void {
    this.finished = this.finished.filter((entry) => entry.requestId !== requestId);
  }

  /** Empties the outbox, in memory. Returns the unanswered entries it held. */
  clear(): OutboxEntry[] {
    const dropped = this.open;
    this.open = [];
    this.finished = [];
    return dropped;
  }

  /**
   * Writes the unanswered entries to the store. Writes happen one after another, each with the
   * entries as they are when it starts. Rejects when the store fails.
   */
  save(): Promise<void> {
    const run = this.writes.then(async () => {
      if (this.open.length === 0) {
        await this.store.delete(this.key);
        return;
      }
      const stored: Stored[] = this.open.map((entry) => ({
        requestId: entry.requestId,
        method: entry.method,
        params: entry.params,
        createdAt: entry.createdAt,
        attempts: entry.attempts,
        firstSentAt: entry.firstSentAt,
      }));
      await this.store.set(this.key, JSON.stringify(stored));
    });
    this.writes = run.catch(() => undefined);
    return run;
  }

  /** Resolves when every write started so far has ended. */
  settled(): Promise<void> {
    return this.writes;
  }
}
