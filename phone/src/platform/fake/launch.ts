import { defineCapability, type Support } from '../capability';
import type { LaunchCapability, LaunchInfo } from '../capabilities/launch';
import { createFakeSupport, type FakeSupport } from './support';

/** The fake's device unless a test says otherwise: an iOS simulator, as in development. */
export const FAKE_IOS_SIMULATOR: LaunchInfo = Object.freeze({
  device: Object.freeze({ platform: 'ios', systemVersion: '26.5', model: 'iPhone 17 Pro' }),
  isSimulator: true,
  hostAddresses: Object.freeze(['127.0.0.1']),
  runtime: Object.freeze({ engine: 'hermes', newArchitecture: true }),
});

export const FAKE_ANDROID_EMULATOR: LaunchInfo = Object.freeze({
  device: Object.freeze({ platform: 'android', systemVersion: '15', model: 'sdk_gphone64_arm64' }),
  isSimulator: true,
  hostAddresses: Object.freeze(['10.0.2.2']),
  runtime: Object.freeze({ engine: 'hermes', newArchitecture: true }),
});

export const FAKE_IPHONE: LaunchInfo = Object.freeze({
  device: Object.freeze({ platform: 'ios', systemVersion: '26.5', model: 'iPhone 17 Pro' }),
  isSimulator: false,
  hostAddresses: Object.freeze([]),
  runtime: Object.freeze({ engine: 'hermes', newArchitecture: true }),
});

export interface FakeLaunch {
  readonly capability: LaunchCapability;
  readonly support: FakeSupport;
}

export function createFakeLaunch(
  info: LaunchInfo = FAKE_IOS_SIMULATOR,
  initial?: Support,
): FakeLaunch {
  const support = createFakeSupport('launch', initial);
  return {
    capability: defineCapability<LaunchCapability>('launch', support.check, { info: () => info }),
    support,
  };
}
