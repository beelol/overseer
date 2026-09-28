import type { Support } from '../capability';
import type { Capabilities, CapabilityName } from '../capabilities';
import type { AppPhase } from '../capabilities/appState';
import type { ColorScheme } from '../capabilities/appearance';
import type { DiscoveryConfig } from '../capabilities/discovery';
import type { LaunchInfo } from '../capabilities/launch';
import type { NetworkState } from '../capabilities/network';
import { createFakeCamera, type FakeCamera } from './camera';
import { createFakeDeviceUnlock, type FakeDeviceUnlock } from './deviceUnlock';
import { createFakeDiscovery, type FakeDiscovery } from './discovery';
import { createFakeHaptics, type FakeHaptics } from './haptics';
import { createFakeLaunch, type FakeLaunch } from './launch';
import {
  createFakeAppState,
  createFakeAppearance,
  createFakeNetwork,
  createFakeReduceMotion,
  type FakeLive,
} from './live';
import { createFakePush, type FakePush } from './push';
import { createFakeRandom, type FakeRandom } from './random';
import {
  createFakeKeyValue,
  createFakeSecretStore,
  type FakeKeyValue,
  type FakeSecretStore,
} from './stores';

export * from './camera';
export * from './deviceUnlock';
export * from './discovery';
export * from './haptics';
export * from './launch';
export * from './live';
export * from './push';
export * from './random';
export * from './stores';
export * from './support';

export interface FakeOptions {
  /** The device to pretend to be. An iOS simulator unless given. */
  readonly launch?: LaunchInfo;
  /** The seed of the random bytes. The same seed gives the same bytes. */
  readonly seed?: number;
  readonly appearance?: ColorScheme;
  readonly appPhase?: AppPhase;
  readonly reduceMotion?: boolean;
  readonly network?: NetworkState;
  readonly discovery?: DiscoveryConfig;
  /** Support answers to start from, for the capabilities named. */
  readonly support?: Partial<Record<CapabilityName, Support>>;
}

/** The controls of each fake: what a test uses to play the system and the owner. */
export interface Fakes {
  readonly secretStore: FakeSecretStore;
  readonly keyValue: FakeKeyValue;
  readonly random: FakeRandom;
  readonly discovery: FakeDiscovery;
  readonly push: FakePush;
  readonly deviceUnlock: FakeDeviceUnlock;
  readonly camera: FakeCamera;
  readonly haptics: FakeHaptics;
  readonly launch: FakeLaunch;
  readonly appearance: FakeLive<Capabilities['appearance'], ColorScheme>;
  readonly reduceMotion: FakeLive<Capabilities['reduceMotion'], boolean>;
  readonly appState: FakeLive<Capabilities['appState'], AppPhase>;
  readonly network: FakeLive<Capabilities['network'], NetworkState>;
}

export interface FakePlatform {
  /** What the app sees: the same interfaces as on a device. */
  readonly capabilities: Capabilities;
  /** What the test holds. */
  readonly fakes: Fakes;
}

/**
 * Every capability, in memory and deterministic: no simulator, no clock, no network.
 * Each call returns a fresh platform that shares nothing with the others.
 */
export function createFakePlatform(options: FakeOptions = {}): FakePlatform {
  const support = options.support ?? {};
  const launch = createFakeLaunch(options.launch, support.launch);
  const keyValue = createFakeKeyValue(support.keyValue);
  const fakes: Fakes = {
    secretStore: createFakeSecretStore(support.secretStore),
    keyValue,
    random: createFakeRandom(options.seed, support.random),
    discovery: createFakeDiscovery({
      launch: launch.capability,
      keyValue: keyValue.capability,
      config: options.discovery,
      support: support.discovery,
    }),
    push: createFakePush(support.push),
    deviceUnlock: createFakeDeviceUnlock(support.deviceUnlock),
    camera: createFakeCamera(launch.capability, support.camera),
    haptics: createFakeHaptics(launch.capability, support.haptics),
    launch,
    appearance: createFakeAppearance(options.appearance, support.appearance),
    reduceMotion: createFakeReduceMotion(options.reduceMotion, support.reduceMotion),
    appState: createFakeAppState(options.appPhase, support.appState),
    network: createFakeNetwork(options.network, support.network),
  };
  const capabilities: Capabilities = Object.freeze({
    secretStore: fakes.secretStore.capability,
    keyValue: fakes.keyValue.capability,
    random: fakes.random.capability,
    discovery: fakes.discovery.capability,
    push: fakes.push.capability,
    deviceUnlock: fakes.deviceUnlock.capability,
    camera: fakes.camera.capability,
    haptics: fakes.haptics.capability,
    launch: fakes.launch.capability,
    appearance: fakes.appearance.capability,
    reduceMotion: fakes.reduceMotion.capability,
    appState: fakes.appState.capability,
    network: fakes.network.capability,
  });
  return { capabilities, fakes };
}
