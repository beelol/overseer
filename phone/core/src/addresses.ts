/** Where the gateway may be: addresses, their order, and the URL of each. */

import { OverseerError } from "./errors.ts";

export const DEFAULT_PORT = 47_810;
export const GATEWAY_PATH = "/v1";

/** A host and a port. The host is an IP address or a host name. */
export interface Address {
  readonly host: string;
  readonly port: number;
}

/** An address whose port may be left out; the port of the pairing is used then. */
export interface AddressInput {
  readonly host: string;
  readonly port?: number;
}

const HOST = /^[A-Za-z0-9._:%-]+$/;

/** True for a host and port that can be tried. */
export function isUsableAddress(address: AddressInput): boolean {
  if (typeof address.host !== "string" || address.host.length < 1 || address.host.length > 255 || !HOST.test(address.host)) return false;
  if (address.port === undefined) return true;
  return Number.isInteger(address.port) && address.port >= 1 && address.port <= 65_535;
}

/** Addresses are the same when host (without case) and port are. */
export function addressKey(address: Address): string {
  return `${address.host.toLowerCase()}|${address.port}`;
}

/** The text form: `host:port`, with brackets around an IPv6 address. */
export function formatAddress(address: Address): string {
  return address.host.includes(":") ? `[${address.host}]:${address.port}` : `${address.host}:${address.port}`;
}

/** The WebSocket URL of the gateway at `address`. */
export function gatewayUrl(address: Address): string {
  const host = address.host.includes(":") ? `[${address.host.replace(/%/g, "%25")}]` : address.host;
  return `ws://${host}:${address.port}${GATEWAY_PATH}`;
}

/**
 * Reads an address a person typed: `host`, `host:port`, `[ipv6]`, `[ipv6]:port` or a bare IPv6
 * address. Throws when it is none of these.
 */
export function parseAddress(text: string, defaultPort: number = DEFAULT_PORT): Address {
  const trimmed = text.trim();
  let host = trimmed;
  let port = defaultPort;
  const bracketed = /^\[([^\]]+)\](?::(\d{1,5}))?$/.exec(trimmed);
  if (bracketed) {
    host = bracketed[1] as string;
    if (bracketed[2] !== undefined) port = Number(bracketed[2]);
  } else {
    const colons = trimmed.split(":").length - 1;
    if (colons === 1) {
      const [name, number] = trimmed.split(":") as [string, string];
      host = name;
      port = /^\d{1,5}$/.test(number) ? Number(number) : Number.NaN;
    }
  }
  const address = { host, port };
  if (!isUsableAddress(address)) throw new OverseerError("not_connected", "this is not an address: use a name or an IP address, with or without a port");
  return address;
}

/** The lists the candidates come from, in the order they are tried. */
export interface CandidateSources {
  /** The address that worked last. */
  readonly last: Address | null;
  /** Addresses found on the network, for the gateway's fingerprint. */
  readonly discovered: readonly AddressInput[];
  /** Addresses of the pairing code. */
  readonly paired: readonly string[];
  /** Addresses the platform adds (`127.0.0.1` on the iOS simulator, `10.0.2.2` on the Android emulator). */
  readonly extras: readonly AddressInput[];
  /** The port of the pairing code, for addresses without one. */
  readonly port: number;
}

/**
 * The addresses to try, in order and without repeats: the last one that worked, the discovered
 * ones, those of the pairing code, then the platform's extras.
 */
export function candidateAddresses(sources: CandidateSources): Address[] {
  const seen = new Set<string>();
  const out: Address[] = [];
  const add = (input: AddressInput): void => {
    if (!isUsableAddress(input)) return;
    const address = { host: input.host, port: input.port ?? sources.port };
    const key = addressKey(address);
    if (seen.has(key)) return;
    seen.add(key);
    out.push(address);
  };
  if (sources.last) add(sources.last);
  for (const address of sources.discovered) add(address);
  for (const host of sources.paired) add({ host });
  for (const address of sources.extras) add(address);
  return out;
}
