/** A small typed event emitter. Listeners are called in the order they were added. */

import type { Log } from "./platform.ts";

/** Maps event names to their listeners. */
export type ListenerMap<Events> = { [K in keyof Events]: (...args: never[]) => unknown };

export class Emitter<Events extends ListenerMap<Events>> {
  private readonly listeners = new Map<keyof Events, Set<Events[keyof Events]>>();
  private readonly log: Log | undefined;

  constructor(log?: Log) {
    this.log = log;
  }

  /** Adds a listener. The returned function removes it. */
  on<K extends keyof Events>(event: K, listener: Events[K]): () => void {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    set.add(listener);
    return () => {
      this.listeners.get(event)?.delete(listener);
    };
  }

  /** Calls every listener. One that throws is logged and does not stop the others. */
  emit<K extends keyof Events>(event: K, ...args: Parameters<Events[K]>): void {
    for (const listener of [...(this.listeners.get(event) ?? [])]) {
      try {
        const result = (listener as (...a: Parameters<Events[K]>) => unknown)(...args);
        if (result instanceof Promise) result.catch(() => this.log?.(`a listener of "${String(event)}" failed`));
      } catch {
        this.log?.(`a listener of "${String(event)}" threw`);
      }
    }
  }

  /** Calls every listener and waits for each before the next. */
  async emitAndWait<K extends keyof Events>(event: K, ...args: Parameters<Events[K]>): Promise<void> {
    for (const listener of [...(this.listeners.get(event) ?? [])]) {
      try {
        await (listener as (...a: Parameters<Events[K]>) => unknown)(...args);
      } catch {
        this.log?.(`a listener of "${String(event)}" failed`);
      }
    }
  }
}
