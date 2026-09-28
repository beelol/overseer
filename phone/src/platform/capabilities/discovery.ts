import type { Capability, Listener, Unsubscribe } from '../capability';
import type { Live } from '../live';

/** Where a gateway may be listening. `host` is an IP address or a host name. */
export interface GatewayAddress {
  readonly host: string;
  readonly port: number;
}

/**
 * An address worth trying, and where it came from:
 * `platform` is an address the platform adds (the host's loopback on simulators),
 * `manual` was typed by the owner.
 */
export interface GatewayCandidate extends GatewayAddress {
  readonly source: 'platform' | 'manual';
}

/** A gateway found by browsing the local network. It is trusted only after the handshake. */
export interface DiscoveredGateway extends GatewayAddress {
  readonly serviceName: string;
  /** The service's TXT record, which carries the gateway key's fingerprint as `fp`. */
  readonly txt: Readonly<Record<string, string>>;
}

export type ManualAddressResult =
  | { readonly ok: true; readonly address: GatewayAddress }
  | { readonly ok: false; readonly reason: string };

/** What discovery is configured with: the service it looks for and the port it assumes. */
export interface DiscoveryConfig {
  /** The Bonjour service type, for example `_overseer._tcp`. */
  readonly serviceType: string;
  /** The port used when a manual address names none, and for the platform's addresses. */
  readonly defaultPort: number;
}

/**
 * Finding the gateway on the local network.
 *
 * `support()` answers for browsing only. Manual addresses and the platform's own addresses always
 * work, on every platform and on simulators.
 */
export interface DiscoveryApi {
  readonly config: DiscoveryConfig;
  /** Addresses to try without browsing: the platform's first, then manual ones, no duplicates. */
  candidates(): readonly GatewayCandidate[];
  /** The manual addresses, kept across launches. */
  readonly manual: Live<readonly GatewayAddress[]>;
  /**
   * Adds an address the owner typed: `host`, `host:port` or `[ipv6]:port`.
   * Adding an address that is already there succeeds and changes nothing.
   */
  addManual(input: string): ManualAddressResult;
  removeManual(address: GatewayAddress): void;
  /**
   * Browses the local network and reports the gateways found, again after each change.
   * Throws `CapabilityUnsupportedError` when `support()` reports unsupported.
   */
  browse(listener: Listener<readonly DiscoveredGateway[]>): Unsubscribe;
}

export type DiscoveryCapability = Capability<'discovery', DiscoveryApi>;
