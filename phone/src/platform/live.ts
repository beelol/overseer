import type { Listener, Unsubscribe } from './capability';

/**
 * A value that changes while the app runs: read it now, or be told when it changes.
 * The appearance, Reduce Motion, the app's state and the network are all `Live` values, so one
 * hook (`useLive`) follows any of them.
 */
export interface Live<T> {
  /** The current value. Returns the same reference until the value changes. */
  get(): T;
  /** Calls `listener` after each change (not for the current value). */
  subscribe(listener: Listener<T>): Unsubscribe;
}

/** A `Live` value together with the way to change it. Implementations keep `set` to themselves. */
export interface LiveSource<T> extends Live<T> {
  /** Stores `next` and tells the listeners, unless it equals the current value. */
  set(next: T): void;
}

export function createLive<T>(
  initial: T,
  equals: (a: T, b: T) => boolean = Object.is,
): LiveSource<T> {
  let current = initial;
  const listeners = new Set<Listener<T>>();
  return {
    get: () => current,
    subscribe(listener) {
      // A wrapper per subscription, so the same function subscribed twice is two subscriptions.
      const entry: Listener<T> = (value) => listener(value);
      listeners.add(entry);
      return () => {
        listeners.delete(entry);
      };
    },
    set(next) {
      if (equals(current, next)) return;
      current = next;
      for (const listener of [...listeners]) listener(current);
    },
  };
}
