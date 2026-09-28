import type { Capability } from '../capability';
import type { AsyncStore, SecretSchema } from '../stores';

/**
 * The system keystore: the Keychain on iOS, the Keystore on Android.
 * Holds the phone's private key and the Mac's public key after pairing. Values never leave the
 * device: they are excluded from backups and from transfer to a new phone.
 */
export interface SecretStoreApi {
  /**
   * A view of the keystore under `namespace`, typed by what it holds.
   *
   * @example
   * type PairingSecrets = { devicePrivateKey: string; macPublicKey: string };
   * const secrets = secretStore.scope<PairingSecrets>('pairing');
   * await secrets.set('devicePrivateKey', encoded);
   */
  scope<Schema extends SecretSchema<Schema>>(namespace: string): AsyncStore<Schema>;
}

export type SecretStoreCapability = Capability<'secretStore', SecretStoreApi>;
