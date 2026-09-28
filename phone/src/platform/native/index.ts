import type { Capabilities } from '../capabilities';
import type { DiscoveryConfig } from '../capabilities/discovery';
import { createDiscovery } from '../discoveryCore';
import { createAppState } from './appState';
import { createAppearance } from './appearance';
import { createCamera } from './camera';
import { createDeviceUnlock } from './deviceUnlock';
import { createDiscoveryBrowser } from './discoveryBrowser';
import { createHaptics } from './haptics';
import { createKeyValue } from './keyValue';
import { createLaunch } from './launch';
import { createNetwork } from './network';
import { createPush } from './push';
import { createRandom } from './random';
import { createReduceMotion } from './reduceMotion';
import { createSecretStore } from './secretStore';

export interface NativeConfig {
  readonly discovery: DiscoveryConfig;
}

/**
 * The capabilities of the device the app runs on. Created once, when the app starts.
 * Files that end in `.ios` or `.android` are chosen by the bundler; nothing here asks which
 * platform it is.
 */
export function createNativeCapabilities(config: NativeConfig): Capabilities {
  const launch = createLaunch();
  const keyValue = createKeyValue();
  return Object.freeze({
    secretStore: createSecretStore(),
    keyValue,
    random: createRandom(),
    discovery: createDiscovery({
      config: config.discovery,
      launch,
      keyValue,
      browser: createDiscoveryBrowser(launch),
    }),
    push: createPush(),
    deviceUnlock: createDeviceUnlock(),
    camera: createCamera(launch),
    haptics: createHaptics(launch),
    launch,
    appearance: createAppearance(),
    reduceMotion: createReduceMotion(),
    appState: createAppState(),
    network: createNetwork(),
  });
}
