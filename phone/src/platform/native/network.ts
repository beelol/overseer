import * as Network from 'expo-network';

import { SUPPORTED, defineCapability } from '../capability';
import {
  NETWORK_UNKNOWN,
  sameNetworkState,
  type NetworkCapability,
  type NetworkKind,
  type NetworkState,
} from '../capabilities/network';
import { createLive } from '../live';

const kinds: Readonly<Partial<Record<Network.NetworkStateType, NetworkKind>>> = {
  [Network.NetworkStateType.NONE]: 'none',
  [Network.NetworkStateType.UNKNOWN]: 'unknown',
  [Network.NetworkStateType.CELLULAR]: 'cellular',
  [Network.NetworkStateType.WIFI]: 'wifi',
  [Network.NetworkStateType.ETHERNET]: 'ethernet',
  [Network.NetworkStateType.VPN]: 'vpn',
};

function toState(state: Network.NetworkState): NetworkState {
  const kind = state.type === undefined ? 'unknown' : (kinds[state.type] ?? 'other');
  return Object.freeze({ connected: state.isConnected === true, kind });
}

/** The device's connection, through expo-network. */
export function createNetwork(): NetworkCapability {
  const network = createLive<NetworkState>(NETWORK_UNKNOWN, sameNetworkState);
  async function refresh(): Promise<NetworkState> {
    network.set(toState(await Network.getNetworkStateAsync()));
    return network.get();
  }
  Network.addNetworkStateListener((state) => network.set(toState(state)));
  refresh().catch(() => undefined);
  return defineCapability<NetworkCapability>('network', async () => SUPPORTED, {
    get: network.get,
    subscribe: network.subscribe,
    refresh,
  });
}
