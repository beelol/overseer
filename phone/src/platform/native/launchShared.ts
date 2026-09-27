import * as Device from 'expo-device';

import { SUPPORTED, defineCapability } from '../capability';
import type {
  LaunchCapability,
  LaunchDevice,
  LaunchInfo,
  LaunchRuntime,
} from '../capabilities/launch';

// Set by the runtime itself: Hermes defines HermesInternal, and the New Architecture runs
// bridgeless with Fabric's UI manager installed.
declare const globalThis: {
  readonly HermesInternal?: object | null;
  readonly RN$Bridgeless?: boolean;
  readonly nativeFabricUIManager?: object | null;
};

function runtime(): LaunchRuntime {
  return {
    engine: globalThis.HermesInternal != null ? 'hermes' : 'other',
    newArchitecture: globalThis.RN$Bridgeless === true && globalThis.nativeFabricUIManager != null,
  };
}

/** What each platform's file supplies; the rest is the same on both. */
interface PlatformLaunch {
  readonly platform: LaunchDevice['platform'];
  /** The address of the Mac as seen from this platform's simulator. */
  readonly simulatorHostAddress: string;
}

export function createLaunchFor({
  platform,
  simulatorHostAddress,
}: PlatformLaunch): LaunchCapability {
  const isSimulator = !Device.isDevice;
  const info: LaunchInfo = Object.freeze({
    device: Object.freeze({
      platform,
      systemVersion: Device.osVersion ?? 'unknown',
      model: Device.modelName ?? 'unknown',
    }),
    isSimulator,
    hostAddresses: Object.freeze(isSimulator ? [simulatorHostAddress] : []),
    runtime: Object.freeze(runtime()),
  });
  return defineCapability<LaunchCapability>('launch', async () => SUPPORTED, { info: () => info });
}
