/**
 * Everything the library needs from the platform, as small interfaces the app injects.
 * The library itself never imports React Native, Expo or Node.
 */

/** Returns `n` bytes from the platform's secure random generator. */
export type RandomSource = (n: number) => Uint8Array;

/** The current time in milliseconds since 1970. */
export type Clock = () => number;

/** Receives diagnostic lines. They never contain keys, secrets or message content. */
export type Log = (message: string) => void;

/** A durable store of strings, for example AsyncStorage or SQLite. */
export interface KeyValueStore {
  /** The value at `key`, or null when there is none. */
  get(key: string): Promise<string | null>;
  set(key: string, value: string): Promise<void>;
  delete(key: string): Promise<void>;
}

/**
 * A store for keys, with the same shape as `KeyValueStore`. The app backs it with the system
 * keystore (Keychain on iOS, Keystore on Android).
 */
export type SecretStore = KeyValueStore;

/** An opaque handle of a running timer. */
export type TimerHandle = unknown;

/** Timers. The default uses the platform's `setTimeout`; tests may inject their own. */
export interface Timers {
  set(callback: () => void, delayMs: number): TimerHandle;
  clear(handle: TimerHandle): void;
}

interface TimerHost {
  setTimeout(callback: () => void, delayMs: number): unknown;
  clearTimeout(handle: unknown): void;
}

/** Timers backed by the global `setTimeout`, which Node and React Native both provide. */
export const platformTimers: Timers = {
  set(callback, delayMs) {
    return (globalThis as unknown as TimerHost).setTimeout(callback, delayMs);
  },
  clear(handle) {
    (globalThis as unknown as TimerHost).clearTimeout(handle);
  },
};

/** A store that lives in memory: the fake for tests, and a stand-in before real storage exists. */
export class MemoryStore implements KeyValueStore {
  private readonly values = new Map<string, string>();

  async get(key: string): Promise<string | null> {
    return this.values.get(key) ?? null;
  }

  async set(key: string, value: string): Promise<void> {
    this.values.set(key, value);
  }

  async delete(key: string): Promise<void> {
    this.values.delete(key);
  }

  /** The keys present now, sorted. For tests. */
  keys(): string[] {
    return [...this.values.keys()].sort();
  }

  /** A copy of everything stored, as the state a restarted app would find. */
  snapshot(): Map<string, string> {
    return new Map(this.values);
  }
}

/** Reads a number of the given random bytes as a fraction in [0, 1). */
export function randomFraction(random: RandomSource): number {
  const bytes = random(4);
  if (bytes.length !== 4) throw new Error("the random source returned the wrong number of bytes");
  const value = (((bytes[0] as number) << 24) | ((bytes[1] as number) << 16) | ((bytes[2] as number) << 8) | (bytes[3] as number)) >>> 0;
  return value / 0x1_0000_0000;
}
