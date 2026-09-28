import type { JsonOf } from './capability';

/** The keys of a store's schema. */
export type KeyOf<Schema> = Extract<keyof Schema, string>;

/** A schema whose values are all strings: what the keystore holds. */
export type SecretSchema<Schema> = { readonly [K in keyof Schema]: string };

/** A schema whose values JSON can carry: what the key-value store holds. */
export type JsonSchema<Schema> = { readonly [K in keyof Schema]: JsonOf<Schema[K]> };

/**
 * An asynchronous store typed by what it holds: `Schema` maps each key to its value's type.
 * Used for the system keystore, where every read may wait on the secure hardware.
 */
export interface AsyncStore<Schema> {
  /** The stored value, or `null` when nothing is stored under `key`. */
  get<K extends KeyOf<Schema>>(key: K): Promise<Schema[K] | null>;
  set<K extends KeyOf<Schema>>(key: K, value: Schema[K]): Promise<void>;
  /** Removes the value. Removing a missing key is not an error. */
  delete<K extends KeyOf<Schema>>(key: K): Promise<void>;
}

/**
 * A synchronous store typed by what it holds. Used for the fast non-secret storage the first
 * screen is drawn from, before anything asynchronous has had a chance to run.
 */
export interface SyncStore<Schema> {
  /** The stored value, or `null` when nothing readable is stored under `key`. */
  get<K extends KeyOf<Schema>>(key: K): Schema[K] | null;
  set<K extends KeyOf<Schema>>(key: K, value: Schema[K]): void;
  /** Removes the value. Removing a missing key is not an error. */
  delete<K extends KeyOf<Schema>>(key: K): void;
  /** The keys that currently hold a value in this scope, sorted. */
  keys(): readonly KeyOf<Schema>[];
}

/** What an asynchronous backend provides: strings under flat keys. */
export interface RawAsyncStore {
  getItem(key: string): Promise<string | null>;
  setItem(key: string, value: string): Promise<void>;
  deleteItem(key: string): Promise<void>;
}

/** What a synchronous backend provides: strings under flat keys. */
export interface RawSyncStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  deleteItem(key: string): void;
  allKeys(): readonly string[];
}

// Both system keystores accept these characters in a key, so every store uses the same rule.
// A namespace has no dot, so the first dot of a full key always ends the namespace.
const NAMESPACE = /^[A-Za-z0-9_-]+$/;
const KEY = /^[A-Za-z0-9._-]+$/;
const SEPARATOR = '.';

function checkName(kind: 'namespace' | 'key', value: string): void {
  if (!(kind === 'namespace' ? NAMESPACE : KEY).test(value)) {
    const dot = kind === 'key' ? ', "."' : '';
    throw new TypeError(
      `store ${kind} ${JSON.stringify(value)} may only contain letters, digits${dot}, "_" and "-"`,
    );
  }
}

function fullKey(namespace: string, key: string): string {
  checkName('key', key);
  return `${namespace}${SEPARATOR}${key}`;
}

/** A typed view of an asynchronous string backend, under one namespace. */
export function scopeAsyncStore<Schema extends SecretSchema<Schema>>(
  raw: RawAsyncStore,
  namespace: string,
): AsyncStore<Schema> {
  checkName('namespace', namespace);
  return {
    // The schema's values are strings, so what was stored under K is a Schema[K].
    get: async <K extends KeyOf<Schema>>(key: K) =>
      (await raw.getItem(fullKey(namespace, key))) as Schema[K] | null,
    set: (key, value) => raw.setItem(fullKey(namespace, key), value),
    delete: (key) => raw.deleteItem(fullKey(namespace, key)),
  };
}

/** A typed view of a synchronous string backend, under one namespace. Values are kept as JSON. */
export function scopeSyncStore<Schema extends JsonSchema<Schema>>(
  raw: RawSyncStore,
  namespace: string,
): SyncStore<Schema> {
  checkName('namespace', namespace);
  const prefix = `${namespace}${SEPARATOR}`;
  return {
    get<K extends KeyOf<Schema>>(key: K): Schema[K] | null {
      const text = raw.getItem(fullKey(namespace, key));
      if (text === null) return null;
      try {
        return JSON.parse(text) as Schema[K];
      } catch {
        // An unreadable value is a missing value: the store is a cache, never the only copy.
        return null;
      }
    },
    set(key, value) {
      raw.setItem(fullKey(namespace, key), JSON.stringify(value));
    },
    delete(key) {
      raw.deleteItem(fullKey(namespace, key));
    },
    keys() {
      return raw
        .allKeys()
        .filter((key) => key.startsWith(prefix))
        .map((key) => key.slice(prefix.length) as KeyOf<Schema>)
        .sort();
    },
  };
}
