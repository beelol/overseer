import type { Capability } from '../capability';

export type UnlockMethod = 'face' | 'fingerprint' | 'iris' | 'passcode';

export type UnlockFailure =
  /** The owner or the system dismissed the prompt. */
  | 'cancelled'
  /** The face, finger or passcode did not match. */
  | 'failed'
  /** Too many failed attempts; the device wants its passcode first. */
  | 'lockedOut'
  /** No passcode, face or fingerprint is set up on the device. */
  | 'notSetUp'
  | 'unavailable';

export type UnlockResult =
  { readonly ok: true } | { readonly ok: false; readonly cause: UnlockFailure };

export interface UnlockRequest {
  /** Shown in the system's prompt: what the unlock is for. */
  readonly reason: string;
}

/**
 * Asking the owner to prove it is them with the device's own unlock: Face ID, Touch ID or the
 * passcode on iOS; fingerprint, face or PIN on Android. Used by the optional app lock and
 * before destructive actions, both off by default.
 */
export interface DeviceUnlockApi {
  /** The ways this device can be unlocked right now. Empty when nothing is set up. */
  methods(): Promise<readonly UnlockMethod[]>;
  /** Shows the system's prompt. Resolves with the outcome; never rejects for a refusal. */
  unlock(request: UnlockRequest): Promise<UnlockResult>;
}

export type DeviceUnlockCapability = Capability<'deviceUnlock', DeviceUnlockApi>;
