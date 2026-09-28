import type { GatewayAddress, ManualAddressResult } from './capabilities/discovery';

const HOST_NAME =
  /^(?=.{1,253}$)([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*$/;
const IPV4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/;
const IPV6 = /^[0-9a-f:.]+(%[a-z0-9._-]+)?$/;
const HOW = 'Type a host or host:port, for example 192.168.1.20 or mac.local:47810.';

function refuse(reason: string): ManualAddressResult {
  return { ok: false, reason };
}

function isIpv4(host: string): boolean {
  const match = IPV4.exec(host);
  return match !== null && match.slice(1).every((part) => Number(part) <= 255);
}

function isIpv6(host: string): boolean {
  return host.includes(':') && IPV6.test(host) && host.split(':').length <= 9;
}

function parsePort(text: string): number | null {
  if (!/^\d{1,5}$/.test(text)) return null;
  const port = Number(text);
  return port >= 1 && port <= 65535 ? port : null;
}

/** Splits what was typed into a host and the text of a port, if one was given. */
function split(input: string): { host: string; port: string | null } | null {
  if (input.startsWith('[')) {
    const end = input.indexOf(']');
    if (end < 0) return null;
    const rest = input.slice(end + 1);
    if (rest !== '' && !rest.startsWith(':')) return null;
    return { host: input.slice(1, end), port: rest === '' ? null : rest.slice(1) };
  }
  const colons = input.split(':').length - 1;
  if (colons === 0) return { host: input, port: null };
  if (colons === 1) {
    const at = input.indexOf(':');
    return { host: input.slice(0, at), port: input.slice(at + 1) };
  }
  // More than one colon and no brackets: a bare IPv6 address without a port.
  return { host: input, port: null };
}

/**
 * Reads an address the owner typed: `host`, `host:port`, `[ipv6]` or `[ipv6]:port`.
 * The same rules on every platform.
 */
export function parseManualAddress(input: string, defaultPort: number): ManualAddressResult {
  const text = input.trim().toLowerCase();
  if (text === '') return refuse(`The address is empty. ${HOW}`);
  if (/\s/.test(text)) return refuse(`An address has no spaces. ${HOW}`);
  if (text.includes('/')) return refuse(`Leave out the scheme and the path. ${HOW}`);

  const parts = split(text);
  if (parts === null) return refuse(`The brackets do not match. ${HOW}`);

  const { host } = parts;
  const bracketed = text.startsWith('[');
  const valid =
    bracketed || host.includes(':') ? isIpv6(host) : isIpv4(host) || HOST_NAME.test(host);
  // Four groups of digits that are not an IPv4 address are a mistake, not a host name.
  if (!valid || (IPV4.test(host) && !isIpv4(host))) {
    return refuse(`"${host}" is not an IP address or a host name. ${HOW}`);
  }

  if (parts.port === null) return { ok: true, address: { host, port: defaultPort } };
  const port = parsePort(parts.port);
  if (port === null) return refuse(`"${parts.port}" is not a port from 1 to 65535. ${HOW}`);
  return { ok: true, address: { host, port } };
}

export function sameAddress(a: GatewayAddress, b: GatewayAddress): boolean {
  return a.host === b.host && a.port === b.port;
}

/** How an address is written for people: brackets around IPv6. */
export function formatAddress(address: GatewayAddress): string {
  const host = address.host.includes(':') ? `[${address.host}]` : address.host;
  return `${host}:${address.port}`;
}
