import type { Capability } from '../capability';
import type { Live } from '../live';

export type NetworkKind = 'wifi' | 'cellular' | 'ethernet' | 'vpn' | 'other' | 'none' | 'unknown';

export interface NetworkState {
  /** The device has a network connection. It says nothing about reaching the Mac. */
  readonly connected: boolean;
  readonly kind: NetworkKind;
}

/**
 * The device's network connection. A change is the cue to try the gateway again.
 *
 * The system answers asynchronously, so `get()` is `{ connected: false, kind: 'unknown' }`
 * until the first answer arrives; `refresh()` resolves with the system's current answer.
 */
export interface NetworkApi extends Live<NetworkState> {
  refresh(): Promise<NetworkState>;
}

export type NetworkCapability = Capability<'network', NetworkApi>;

export const NETWORK_UNKNOWN: NetworkState = Object.freeze({ connected: false, kind: 'unknown' });

export function sameNetworkState(a: NetworkState, b: NetworkState): boolean {
  return a.connected === b.connected && a.kind === b.kind;
}
