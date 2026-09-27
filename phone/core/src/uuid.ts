/** Request ids: a UUID (version 4) per action, made from the injected random source. */

import { bytesToHex } from "./bytes.ts";
import type { RandomSource } from "./platform.ts";

/** A random UUID, version 4, in the usual text form. */
export function uuidV4(random: RandomSource): string {
  const bytes = new Uint8Array(random(16));
  if (bytes.length !== 16) throw new Error("the random source returned the wrong number of bytes");
  bytes[6] = ((bytes[6] as number) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] as number) & 0x3f) | 0x80;
  const hex = bytesToHex(bytes);
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** True for a request id the gateway accepts: 8 to 64 ASCII letters, digits and hyphens. */
export function isRequestId(text: string): boolean {
  return /^[A-Za-z0-9-]{8,64}$/.test(text);
}
