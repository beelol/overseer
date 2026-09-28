/** RFC 4648 base32 without padding, as the pairing code uses it. */

const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/** Why base32 text could not be decoded. */
export type Base32Problem = "character" | "length" | "trailing_bits";

/** Base32 text that cannot be decoded. The message never repeats the text. */
export class Base32Error extends Error {
  readonly problem: Base32Problem;

  constructor(problem: Base32Problem, message: string) {
    super(message);
    this.name = "Base32Error";
    this.problem = problem;
  }
}

/** Uppercase base32 of `bytes`, without padding. */
export function base32Encode(bytes: Uint8Array): string {
  let out = "";
  let buffer = 0;
  let bits = 0;
  for (let i = 0; i < bytes.length; i++) {
    buffer = (buffer << 8) | (bytes[i] as number);
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      out += ALPHABET.charAt((buffer >> bits) & 31);
    }
    buffer &= (1 << bits) - 1;
  }
  if (bits > 0) out += ALPHABET.charAt((buffer << (5 - bits)) & 31);
  return out;
}

/**
 * Decodes unpadded base32 of either case. Strict: a character outside the alphabet, a length no
 * encoder produces, and leftover bits that are not zero are all errors, so a mistyped code is
 * noticed here and not as a failed handshake.
 */
export function base32Decode(text: string): Uint8Array {
  const remainder = text.length % 8;
  if (remainder === 1 || remainder === 3 || remainder === 6) {
    throw new Base32Error("length", "the text has a length that base32 cannot have");
  }
  const out = new Uint8Array(Math.floor((text.length * 5) / 8));
  let buffer = 0;
  let bits = 0;
  let n = 0;
  for (let i = 0; i < text.length; i++) {
    let code = text.charCodeAt(i);
    if (code >= 97 && code <= 122) code -= 32;
    let value: number;
    if (code >= 65 && code <= 90) value = code - 65;
    else if (code >= 50 && code <= 55) value = code - 24;
    else throw new Base32Error("character", "the text has a character that is not base32");
    buffer = (buffer << 5) | value;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out[n++] = (buffer >> bits) & 0xff;
    }
    buffer &= (1 << bits) - 1;
  }
  if (buffer !== 0) throw new Base32Error("trailing_bits", "the text ends with bits that no encoder writes");
  return out;
}
