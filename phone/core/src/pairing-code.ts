/**
 * The pairing code: the text of the QR code and of the typed code.
 *
 * `OVSR1-` followed by unpadded RFC 4648 base32 of
 *
 *     byte 0        version, 0x01
 *     bytes 1..32   the gateway's static public key
 *     bytes 33..48  the pairing secret, 16 random bytes
 *     bytes 49..50  the port, big endian
 *     byte 51       number of addresses
 *     then, each    1 byte length and the address as ASCII (an IP address or a host name)
 *
 * The code holds the pairing secret. No error from this module repeats any part of it.
 */

import { Base32Error, base32Decode, base32Encode } from "./base32.ts";
import { copyBytes, utf8Encode } from "./bytes.ts";
import { OverseerError } from "./errors.ts";

export const PAIRING_CODE_PREFIX = "OVSR1-";
export const PAIRING_CODE_VERSION = 0x01;
export const PAIRING_SECRET_LENGTH = 16;

const KEY_LENGTH = 32;
const FIXED_LENGTH = 1 + KEY_LENGTH + PAIRING_SECRET_LENGTH + 2 + 1;
const ADDRESS = /^[A-Za-z0-9._:%-]+$/;

/** What a pairing code carries. */
export interface PairingCode {
  /** The gateway's static public key, 32 bytes. */
  readonly gatewayPublicKey: Uint8Array;
  /** The pairing secret, 16 bytes. It works once and for two minutes. */
  readonly secret: Uint8Array;
  /** The gateway's port. */
  readonly port: number;
  /** Addresses to try, in order: IP addresses or host names. */
  readonly addresses: readonly string[];
}

/** What is wrong with a pairing code. */
export type PairingCodeProblem =
  /** It does not start with `OVSR`. */
  | "not_a_pairing_code"
  /** It is a pairing code of a version this app does not know. */
  | "unsupported_version"
  /** It has a character that a pairing code never has. */
  | "bad_character"
  /** It is cut short, or has a length no code has. */
  | "wrong_length"
  /** It has bytes after its last address. */
  | "trailing_bytes"
  /** An address in it is empty or not an IP address or host name. */
  | "bad_address"
  /** Its port is zero. */
  | "bad_port"
  /** Encoding only: a value does not fit the layout. */
  | "cannot_encode";

/** A pairing code that cannot be read or written. `problem` says why. */
export class PairingCodeError extends OverseerError {
  readonly problem: PairingCodeProblem;

  constructor(problem: PairingCodeProblem, message: string) {
    super("bad_pairing_code", message);
    this.name = "PairingCodeError";
    this.problem = problem;
  }
}

/** Options for writing a code. */
export interface EncodeOptions {
  /** Puts a hyphen after every `group` characters, for a code that is typed. 0 means none. */
  readonly group?: number;
}

/** Writes a pairing code. */
export function encodePairingCode(code: PairingCode, options: EncodeOptions = {}): string {
  if (code.gatewayPublicKey.length !== KEY_LENGTH) throw new PairingCodeError("cannot_encode", "the gateway key must be 32 bytes");
  if (code.secret.length !== PAIRING_SECRET_LENGTH) throw new PairingCodeError("cannot_encode", "the pairing secret must be 16 bytes");
  if (!Number.isInteger(code.port) || code.port < 1 || code.port > 65_535) throw new PairingCodeError("cannot_encode", "the port must be 1 to 65535");
  if (code.addresses.length > 255) throw new PairingCodeError("cannot_encode", "a code holds at most 255 addresses");
  const addresses = code.addresses.map((address) => {
    if (address.length < 1 || address.length > 255 || !ADDRESS.test(address)) {
      throw new PairingCodeError("cannot_encode", "an address must be an IP address or a host name of 1 to 255 ASCII characters");
    }
    return utf8Encode(address);
  });
  let length = FIXED_LENGTH;
  for (const address of addresses) length += 1 + address.length;
  const bytes = new Uint8Array(length);
  bytes[0] = PAIRING_CODE_VERSION;
  bytes.set(code.gatewayPublicKey, 1);
  bytes.set(code.secret, 1 + KEY_LENGTH);
  bytes[49] = code.port >> 8;
  bytes[50] = code.port & 0xff;
  bytes[51] = addresses.length;
  let offset = FIXED_LENGTH;
  for (const address of addresses) {
    bytes[offset++] = address.length;
    bytes.set(address, offset);
    offset += address.length;
  }
  const text = base32Encode(bytes);
  bytes.fill(0);
  const group = options.group ?? 0;
  if (group <= 0) return PAIRING_CODE_PREFIX + text;
  const groups: string[] = [];
  for (let i = 0; i < text.length; i += group) groups.push(text.slice(i, i + group));
  return PAIRING_CODE_PREFIX + groups.join("-");
}

/**
 * Reads a pairing code as scanned or typed. Letters of either case, spaces, line breaks and
 * extra hyphens are accepted. Everything else that is not exactly a code is an error.
 */
export function decodePairingCode(text: string): PairingCode {
  const compact = text.replace(/[\s-]+/g, "").toUpperCase();
  if (!compact.startsWith("OVSR")) throw new PairingCodeError("not_a_pairing_code", "this is not an Overseer pairing code");
  const version = compact.charAt(4);
  if (version !== "1") {
    if (version >= "0" && version <= "9") throw new PairingCodeError("unsupported_version", "this pairing code is of a version this app does not know; update the app");
    throw new PairingCodeError("not_a_pairing_code", "this is not an Overseer pairing code");
  }
  let bytes: Uint8Array;
  try {
    bytes = base32Decode(compact.slice(5));
  } catch (error) {
    if (error instanceof Base32Error && error.problem === "character") {
      throw new PairingCodeError("bad_character", "the code has a character that a pairing code never has; it uses the letters A to Z and the digits 2 to 7");
    }
    throw new PairingCodeError("wrong_length", "the code is incomplete or mistyped");
  }
  try {
    return parse(bytes);
  } finally {
    bytes.fill(0);
  }
}

function parse(bytes: Uint8Array): PairingCode {
  if (bytes.length < 1) throw new PairingCodeError("wrong_length", "the code is incomplete");
  if (bytes[0] !== PAIRING_CODE_VERSION) throw new PairingCodeError("unsupported_version", "this pairing code is of a version this app does not know; update the app");
  if (bytes.length < FIXED_LENGTH) throw new PairingCodeError("wrong_length", "the code is incomplete");
  const gatewayPublicKey = copyBytes(bytes.subarray(1, 1 + KEY_LENGTH));
  const secret = copyBytes(bytes.subarray(1 + KEY_LENGTH, 1 + KEY_LENGTH + PAIRING_SECRET_LENGTH));
  const port = ((bytes[49] as number) << 8) | (bytes[50] as number);
  if (port === 0) throw new PairingCodeError("bad_port", "the code has no port");
  const count = bytes[51] as number;
  const addresses: string[] = [];
  let offset = FIXED_LENGTH;
  for (let i = 0; i < count; i++) {
    if (offset >= bytes.length) throw new PairingCodeError("wrong_length", "the code is incomplete: an address is missing");
    const length = bytes[offset++] as number;
    if (length === 0) throw new PairingCodeError("bad_address", "the code has an empty address");
    if (offset + length > bytes.length) throw new PairingCodeError("wrong_length", "the code is incomplete: an address is cut short");
    let address = "";
    for (let k = 0; k < length; k++) address += String.fromCharCode(bytes[offset + k] as number);
    if (!ADDRESS.test(address)) throw new PairingCodeError("bad_address", "the code has an address that is not an IP address or a host name");
    addresses.push(address);
    offset += length;
  }
  if (offset !== bytes.length) throw new PairingCodeError("trailing_bytes", "the code has extra content after its last address");
  return { gatewayPublicKey, secret, port, addresses };
}
