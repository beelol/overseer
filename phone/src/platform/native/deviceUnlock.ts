import * as LocalAuthentication from 'expo-local-authentication';

import { SUPPORTED, defineCapability, unsupported } from '../capability';
import type {
  DeviceUnlockCapability,
  UnlockFailure,
  UnlockMethod,
} from '../capabilities/deviceUnlock';

const biometric: Readonly<Record<LocalAuthentication.AuthenticationType, UnlockMethod>> = {
  [LocalAuthentication.AuthenticationType.FINGERPRINT]: 'fingerprint',
  [LocalAuthentication.AuthenticationType.FACIAL_RECOGNITION]: 'face',
  [LocalAuthentication.AuthenticationType.IRIS]: 'iris',
};

function toFailure(error: LocalAuthentication.LocalAuthenticationError): UnlockFailure {
  switch (error) {
    case 'user_cancel':
    case 'app_cancel':
    case 'system_cancel':
    case 'user_fallback':
    case 'timeout':
      return 'cancelled';
    case 'authentication_failed':
      return 'failed';
    case 'lockout':
      return 'lockedOut';
    case 'not_enrolled':
    case 'passcode_not_set':
      return 'notSetUp';
    default:
      return 'unavailable';
  }
}

async function methods(): Promise<readonly UnlockMethod[]> {
  const level = await LocalAuthentication.getEnrolledLevelAsync();
  if (level === LocalAuthentication.SecurityLevel.NONE) return [];
  const found: UnlockMethod[] = [];
  if (level !== LocalAuthentication.SecurityLevel.SECRET) {
    for (const type of await LocalAuthentication.supportedAuthenticationTypesAsync()) {
      found.push(biometric[type]);
    }
  }
  // Whoever enrolled a face or a finger also has a passcode, PIN or pattern.
  found.push('passcode');
  return found;
}

/** Face ID, Touch ID and the passcode; fingerprint, face and PIN: through expo-local-authentication. */
export function createDeviceUnlock(): DeviceUnlockCapability {
  return defineCapability<DeviceUnlockCapability>(
    'deviceUnlock',
    async () => {
      try {
        return (await methods()).length > 0
          ? SUPPORTED
          : unsupported('No passcode, face or fingerprint is set up on this device.');
      } catch {
        return unsupported('The device could not say how it is unlocked.');
      }
    },
    {
      methods,
      async unlock({ reason }) {
        const result = await LocalAuthentication.authenticateAsync({
          promptMessage: reason,
          disableDeviceFallback: false,
        });
        return result.success ? { ok: true } : { ok: false, cause: toFailure(result.error) };
      },
    },
  );
}
