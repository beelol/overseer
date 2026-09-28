import * as SecureStore from 'expo-secure-store';

import { SUPPORTED, defineCapability, unsupported } from '../capability';
import type { SecretStoreCapability } from '../capabilities/secretStore';
import { scopeAsyncStore, type RawAsyncStore } from '../stores';

// Readable after the first unlock, so a reconnect in the background works; never copied to
// another device or into a backup. Android's Keystore has no such choice and ignores it.
const options: SecureStore.SecureStoreOptions = {
  keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
};

const raw: RawAsyncStore = {
  getItem: (key) => SecureStore.getItemAsync(key, options),
  setItem: (key, value) => SecureStore.setItemAsync(key, value, options),
  deleteItem: (key) => SecureStore.deleteItemAsync(key, options),
};

/** The Keychain on iOS and the Keystore on Android, both through expo-secure-store. */
export function createSecretStore(): SecretStoreCapability {
  return defineCapability<SecretStoreCapability>(
    'secretStore',
    async () => {
      try {
        return (await SecureStore.isAvailableAsync())
          ? SUPPORTED
          : unsupported('The system keystore is not available on this device.');
      } catch {
        return unsupported('The system keystore could not be reached.');
      }
    },
    { scope: (namespace) => scopeAsyncStore(raw, namespace) },
  );
}
