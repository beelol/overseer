import Storage from 'expo-sqlite/kv-store';

import { SUPPORTED, defineCapability } from '../capability';
import type { KeyValueCapability } from '../capabilities/keyValue';
import { scopeSyncStore, type RawSyncStore } from '../stores';

const raw: RawSyncStore = {
  getItem: (key) => Storage.getItemSync(key),
  setItem: (key, value) => Storage.setItemSync(key, value),
  deleteItem: (key) => {
    Storage.removeItemSync(key);
  },
  allKeys: () => Storage.getAllKeysSync(),
};

/** SQLite in the app's own files, through expo-sqlite's key-value store. Synchronous. */
export function createKeyValue(): KeyValueCapability {
  return defineCapability<KeyValueCapability>('keyValue', async () => SUPPORTED, {
    scope: (namespace) => scopeSyncStore(raw, namespace),
  });
}
