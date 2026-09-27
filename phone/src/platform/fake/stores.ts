import { defineCapability, type Support } from '../capability';
import type { KeyValueCapability } from '../capabilities/keyValue';
import type { SecretStoreCapability } from '../capabilities/secretStore';
import { scopeAsyncStore, scopeSyncStore, type JsonSchema, type SecretSchema } from '../stores';
import { createFakeSupport, type FakeSupport } from './support';

export interface FakeSecretStore {
  readonly capability: SecretStoreCapability;
  readonly support: FakeSupport;
  /** Everything stored, under its full key (`namespace.key`). */
  readonly items: Map<string, string>;
}

/** A keystore in memory. It uses the same typed scopes and key rules as the real one. */
export function createFakeSecretStore(initial?: Support): FakeSecretStore {
  const support = createFakeSupport('secretStore', initial);
  const items = new Map<string, string>();
  const capability = defineCapability<SecretStoreCapability>('secretStore', support.check, {
    scope: <Schema extends SecretSchema<Schema>>(namespace: string) =>
      scopeAsyncStore<Schema>(
        {
          async getItem(key) {
            support.require();
            return items.get(key) ?? null;
          },
          async setItem(key, value) {
            support.require();
            items.set(key, value);
          },
          async deleteItem(key) {
            support.require();
            items.delete(key);
          },
        },
        namespace,
      ),
  });
  return { capability, support, items };
}

export interface FakeKeyValue {
  readonly capability: KeyValueCapability;
  readonly support: FakeSupport;
  /** Everything stored, as the JSON text kept under its full key (`namespace.key`). */
  readonly items: Map<string, string>;
}

/** Key-value storage in memory, with the same typed scopes and JSON encoding as the real one. */
export function createFakeKeyValue(initial?: Support): FakeKeyValue {
  const support = createFakeSupport('keyValue', initial);
  const items = new Map<string, string>();
  const capability = defineCapability<KeyValueCapability>('keyValue', support.check, {
    scope: <Schema extends JsonSchema<Schema>>(namespace: string) =>
      scopeSyncStore<Schema>(
        {
          getItem(key) {
            support.require();
            return items.get(key) ?? null;
          },
          setItem(key, value) {
            support.require();
            items.set(key, value);
          },
          deleteItem(key) {
            support.require();
            items.delete(key);
          },
          allKeys() {
            support.require();
            return [...items.keys()];
          },
        },
        namespace,
      ),
  });
  return { capability, support, items };
}
