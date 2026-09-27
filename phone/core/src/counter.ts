/**
 * The handshake counter: `max(current time in ms, last counter used + 1)`, stored before it is
 * used. The gateway refuses a counter that is not higher than the last one it accepted, so a
 * recorded handshake cannot be replayed. The counter only ever goes up, also when the clock of
 * the phone goes back and across restarts of the app.
 */

import { OverseerError } from "./errors.ts";
import type { Clock, KeyValueStore } from "./platform.ts";

export class HandshakeCounter {
  private readonly store: KeyValueStore;
  private readonly key: string;
  private readonly now: Clock;
  private highest = 0;
  private queue: Promise<unknown> = Promise.resolve();

  constructor(store: KeyValueStore, key: string, now: Clock) {
    this.store = store;
    this.key = key;
    this.now = now;
  }

  /**
   * The next counter. It is in the store when the promise resolves. Calls are served one at a
   * time, so two handshakes never get the same value.
   */
  next(): Promise<number> {
    const run = this.queue.then(() => this.allocate());
    this.queue = run.catch(() => undefined);
    return run;
  }

  private async allocate(): Promise<number> {
    const stored = await this.store.get(this.key);
    const parsed = stored === null ? 0 : Number(stored);
    if (Number.isSafeInteger(parsed) && parsed > this.highest) this.highest = parsed;
    const value = Math.max(Math.floor(this.now()), this.highest + 1);
    if (!Number.isSafeInteger(value) || value < 1) throw new OverseerError("storage", "the handshake counter cannot go higher");
    await this.store.set(this.key, String(value));
    this.highest = value;
    return value;
  }
}
