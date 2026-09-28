import type { Capability } from '../capability';
import type { AppStateCapability } from './appState';
import type { AppearanceCapability } from './appearance';
import type { CameraCapability } from './camera';
import type { DeviceUnlockCapability } from './deviceUnlock';
import type { DiscoveryCapability } from './discovery';
import type { HapticsCapability } from './haptics';
import type { KeyValueCapability } from './keyValue';
import type { LaunchCapability } from './launch';
import type { NetworkCapability } from './network';
import type { PushCapability } from './push';
import type { RandomCapability } from './random';
import type { ReduceMotionCapability } from './reduceMotion';
import type { SecretStoreCapability } from './secretStore';

export type * from './appState';
export type * from './appearance';
export type * from './camera';
export type * from './deviceUnlock';
export type * from './discovery';
export type * from './haptics';
export type * from './keyValue';
export type * from './launch';
export type * from './network';
export type * from './push';
export type * from './random';
export type * from './reduceMotion';
export type * from './secretStore';

/** Every capability of the platform layer, by name. */
export interface Capabilities {
  readonly secretStore: SecretStoreCapability;
  readonly keyValue: KeyValueCapability;
  readonly random: RandomCapability;
  readonly discovery: DiscoveryCapability;
  readonly push: PushCapability;
  readonly deviceUnlock: DeviceUnlockCapability;
  readonly camera: CameraCapability;
  readonly haptics: HapticsCapability;
  readonly launch: LaunchCapability;
  readonly appearance: AppearanceCapability;
  readonly reduceMotion: ReduceMotionCapability;
  readonly appState: AppStateCapability;
  readonly network: NetworkCapability;
}

export type CapabilityName = keyof Capabilities;

/** The capability names in the order the README's table lists them. */
export const CAPABILITY_NAMES = [
  'secretStore',
  'keyValue',
  'random',
  'discovery',
  'push',
  'deviceUnlock',
  'camera',
  'haptics',
  'launch',
  'appearance',
  'reduceMotion',
  'appState',
  'network',
] as const satisfies readonly CapabilityName[];

// Compile-time checks: every capability is filed under its own name, and the list above
// misses none.
type FiledUnderOwnName = {
  [Name in CapabilityName]: Capabilities[Name] extends Capability<Name, object> ? true : never;
};
type Missing = Exclude<CapabilityName, (typeof CAPABILITY_NAMES)[number]>;
const filedUnderOwnName: FiledUnderOwnName[CapabilityName] = true;
const nothingMissing: [Missing] extends [never] ? true : never = true;
void filedUnderOwnName;
void nothingMissing;
