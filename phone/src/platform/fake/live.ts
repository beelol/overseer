import { defineCapability, type Support } from '../capability';
import type { AppPhase, AppStateCapability } from '../capabilities/appState';
import type { AppearanceCapability, ColorScheme } from '../capabilities/appearance';
import {
  NETWORK_UNKNOWN,
  sameNetworkState,
  type NetworkCapability,
  type NetworkState,
} from '../capabilities/network';
import type { ReduceMotionCapability } from '../capabilities/reduceMotion';
import { createLive } from '../live';
import { createFakeSupport, type FakeSupport } from './support';

/** A fake of a capability that is a `Live` value: the test sets what the system would report. */
export interface FakeLive<Cap, Value> {
  readonly capability: Cap;
  readonly support: FakeSupport;
  set(value: Value): void;
}

export function createFakeAppearance(
  initial: ColorScheme = 'light',
  support?: Support,
): FakeLive<AppearanceCapability, ColorScheme> {
  const fakeSupport = createFakeSupport('appearance', support);
  const scheme = createLive<ColorScheme>(initial);
  return {
    capability: defineCapability<AppearanceCapability>('appearance', fakeSupport.check, {
      get: scheme.get,
      subscribe: scheme.subscribe,
    }),
    support: fakeSupport,
    set: scheme.set,
  };
}

export function createFakeAppState(
  initial: AppPhase = 'foreground',
  support?: Support,
): FakeLive<AppStateCapability, AppPhase> {
  const fakeSupport = createFakeSupport('appState', support);
  const phase = createLive<AppPhase>(initial);
  return {
    capability: defineCapability<AppStateCapability>('appState', fakeSupport.check, {
      get: phase.get,
      subscribe: phase.subscribe,
    }),
    support: fakeSupport,
    set: phase.set,
  };
}

/**
 * Like the real ones, the two fakes below start from the value used before the system has
 * answered (`false`, unknown) and take the system's value on the first `refresh()`.
 */
export function createFakeReduceMotion(
  system = false,
  support?: Support,
): FakeLive<ReduceMotionCapability, boolean> {
  const fakeSupport = createFakeSupport('reduceMotion', support);
  const reduced = createLive(false);
  let reported = system;
  return {
    capability: defineCapability<ReduceMotionCapability>('reduceMotion', fakeSupport.check, {
      get: reduced.get,
      subscribe: reduced.subscribe,
      async refresh() {
        reduced.set(reported);
        return reduced.get();
      },
    }),
    support: fakeSupport,
    set(value) {
      reported = value;
      reduced.set(value);
    },
  };
}

export const FAKE_WIFI: NetworkState = Object.freeze({ connected: true, kind: 'wifi' });

export function createFakeNetwork(
  system: NetworkState = FAKE_WIFI,
  support?: Support,
): FakeLive<NetworkCapability, NetworkState> {
  const fakeSupport = createFakeSupport('network', support);
  const network = createLive<NetworkState>(NETWORK_UNKNOWN, sameNetworkState);
  let reported = system;
  return {
    capability: defineCapability<NetworkCapability>('network', fakeSupport.check, {
      get: network.get,
      subscribe: network.subscribe,
      async refresh() {
        network.set(reported);
        return network.get();
      },
    }),
    support: fakeSupport,
    set(value) {
      reported = Object.freeze({ ...value });
      network.set(reported);
    },
  };
}
