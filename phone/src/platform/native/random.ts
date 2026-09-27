import * as Crypto from 'expo-crypto';

import { SUPPORTED, defineCapability } from '../capability';
import { checkByteCount, type RandomCapability } from '../capabilities/random';

/** SecRandomCopyBytes on iOS and SecureRandom on Android, both through expo-crypto. */
export function createRandom(): RandomCapability {
  return defineCapability<RandomCapability>('random', async () => SUPPORTED, {
    bytes(length) {
      checkByteCount(length);
      return Crypto.getRandomBytes(length);
    },
    uuid: () => Crypto.randomUUID().toLowerCase(),
  });
}
