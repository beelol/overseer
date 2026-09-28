/**
 * A value that is written to the store behind the code that changes it. Writes never overlap
 * and never go back: while one write runs, only the newest value waits to be written next.
 */

import type { KeyValueStore, Log } from "./platform.ts";

export class SavedValue {
  private readonly store: KeyValueStore;
  private readonly key: string;
  private readonly log: Log | undefined;
  private waiting: { value: string | null } | null = null;
  private writing: Promise<void> | null = null;

  constructor(store: KeyValueStore, key: string, log?: Log) {
    this.store = store;
    this.key = key;
    this.log = log;
  }

  /** Reads the stored value. */
  load(): Promise<string | null> {
    return this.store.get(this.key);
  }

  /** Stores `value` soon; null deletes. */
  set(value: string | null): void {
    this.waiting = { value };
    if (!this.writing) this.writing = this.drain();
  }

  /** Resolves when everything set so far is in the store. */
  flush(): Promise<void> {
    return this.writing ?? Promise.resolve();
  }

  private async drain(): Promise<void> {
    while (this.waiting) {
      const { value } = this.waiting;
      this.waiting = null;
      try {
        if (value === null) await this.store.delete(this.key);
        else await this.store.set(this.key, value);
      } catch {
        this.log?.(`could not store ${this.key}`);
      }
    }
    this.writing = null;
  }
}
