import type { Capability } from '../capability';

/** Random values from the system's secure generator: key material, nonces, request ids. */
export interface RandomApi {
  /** `length` random bytes. `length` is a whole number from 0 to 1024. */
  bytes(length: number): Uint8Array;
  /** A random version 4 UUID in lower case, for request ids. */
  uuid(): string;
}

export type RandomCapability = Capability<'random', RandomApi>;

/** Formats 16 random bytes as a version 4 UUID. Shared by implementations that start from bytes. */
export function uuidFromBytes(bytes: Uint8Array): string {
  if (bytes.length !== 16) throw new RangeError('a UUID needs exactly 16 bytes');
  const copy = Uint8Array.from(bytes);
  copy[6] = ((copy[6] ?? 0) & 0x0f) | 0x40;
  copy[8] = ((copy[8] ?? 0) & 0x3f) | 0x80;
  const hex = Array.from(copy, (byte) => byte.toString(16).padStart(2, '0')).join('');
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20),
  ].join('-');
}

export function checkByteCount(length: number): void {
  if (!Number.isInteger(length) || length < 0 || length > 1024) {
    throw new RangeError(`random.bytes needs a whole number from 0 to 1024, got ${length}`);
  }
}
