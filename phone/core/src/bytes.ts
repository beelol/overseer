/**
 * Byte helpers shared by the whole library. Pure functions: nothing here touches the platform
 * except an optional, feature-tested use of `TextEncoder` and `TextDecoder` for speed.
 */

const HEX = "0123456789abcdef";

/** Joins byte arrays into one new array. */
export function concatBytes(...parts: readonly Uint8Array[]): Uint8Array {
  let length = 0;
  for (const part of parts) length += part.length;
  const out = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

/** Lowercase hex of `bytes`. */
export function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i] as number;
    out += HEX.charAt(b >> 4) + HEX.charAt(b & 15);
  }
  return out;
}

function hexDigit(code: number): number {
  if (code >= 48 && code <= 57) return code - 48;
  if (code >= 97 && code <= 102) return code - 87;
  if (code >= 65 && code <= 70) return code - 55;
  return -1;
}

/** Parses hex of either case. Throws on an odd length or a character that is not hex. */
export function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error("hex text has an odd length");
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    const hi = hexDigit(hex.charCodeAt(2 * i));
    const lo = hexDigit(hex.charCodeAt(2 * i + 1));
    if (hi < 0 || lo < 0) throw new Error("hex text has a character that is not hex");
    out[i] = (hi << 4) | lo;
  }
  return out;
}

/**
 * Compares two byte arrays in time that depends only on their lengths, never on where they
 * differ. Use it for keys, tags and fingerprints.
 */
export function equalBytes(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= (a[i] as number) ^ (b[i] as number);
  return diff === 0;
}

/** Overwrites secret bytes with zeros. Best effort: JavaScript may have copied them. */
export function wipe(...arrays: readonly (Uint8Array | null | undefined)[]): void {
  for (const array of arrays) array?.fill(0);
}

/** A copy that owns its memory, so later changes to `bytes` do not reach it. */
export function copyBytes(bytes: Uint8Array): Uint8Array {
  return new Uint8Array(bytes);
}

/** UTF-8 encoding in plain JavaScript. A lone surrogate becomes U+FFFD, as `TextEncoder` does. */
export function utf8EncodePure(text: string): Uint8Array {
  let out = new Uint8Array(Math.max(16, text.length + (text.length >> 1)));
  let n = 0;
  for (let i = 0; i < text.length; i++) {
    let code = text.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = i + 1 < text.length ? text.charCodeAt(i + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        code = 0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00);
        i++;
      } else {
        code = 0xfffd;
      }
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      code = 0xfffd;
    }
    if (n + 4 > out.length) {
      const grown = new Uint8Array(out.length * 2 + 4);
      grown.set(out);
      out = grown;
    }
    if (code < 0x80) {
      out[n++] = code;
    } else if (code < 0x800) {
      out[n++] = 0xc0 | (code >> 6);
      out[n++] = 0x80 | (code & 0x3f);
    } else if (code < 0x10000) {
      out[n++] = 0xe0 | (code >> 12);
      out[n++] = 0x80 | ((code >> 6) & 0x3f);
      out[n++] = 0x80 | (code & 0x3f);
    } else {
      out[n++] = 0xf0 | (code >> 18);
      out[n++] = 0x80 | ((code >> 12) & 0x3f);
      out[n++] = 0x80 | ((code >> 6) & 0x3f);
      out[n++] = 0x80 | (code & 0x3f);
    }
  }
  return out.slice(0, n);
}

const INVALID_UTF8 = "the bytes are not valid UTF-8";

/**
 * Strict UTF-8 decoding in plain JavaScript. Throws on truncated, overlong or out-of-range
 * sequences and on encoded surrogates. A leading byte order mark is kept.
 */
export function utf8DecodePure(bytes: Uint8Array): string {
  const units: number[] = [];
  let out = "";
  let i = 0;
  const end = bytes.length;
  while (i < end) {
    const b0 = bytes[i++] as number;
    let code: number;
    if (b0 < 0x80) {
      code = b0;
    } else {
      let extra: number;
      let min: number;
      if (b0 >= 0xc2 && b0 <= 0xdf) {
        extra = 1;
        min = 0x80;
        code = b0 & 0x1f;
      } else if (b0 >= 0xe0 && b0 <= 0xef) {
        extra = 2;
        min = 0x800;
        code = b0 & 0x0f;
      } else if (b0 >= 0xf0 && b0 <= 0xf4) {
        extra = 3;
        min = 0x10000;
        code = b0 & 0x07;
      } else {
        throw new Error(INVALID_UTF8);
      }
      if (i + extra > end) throw new Error(INVALID_UTF8);
      for (let k = 0; k < extra; k++) {
        const b = bytes[i++] as number;
        if ((b & 0xc0) !== 0x80) throw new Error(INVALID_UTF8);
        code = (code << 6) | (b & 0x3f);
      }
      if (code < min || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) throw new Error(INVALID_UTF8);
    }
    if (code >= 0x10000) {
      const v = code - 0x10000;
      units.push(0xd800 + (v >> 10), 0xdc00 + (v & 0x3ff));
    } else {
      units.push(code);
    }
    if (units.length >= 8192) {
      out += String.fromCharCode.apply(null, units);
      units.length = 0;
    }
  }
  if (units.length > 0) out += String.fromCharCode.apply(null, units);
  return out;
}

interface Encoder {
  encode(text: string): Uint8Array;
}

interface Decoder {
  decode(bytes: Uint8Array): string;
}

interface TextCodecHost {
  TextEncoder?: new () => Encoder;
  TextDecoder?: new (label: string, options: { fatal: boolean; ignoreBOM: boolean }) => Decoder;
}

interface Codecs {
  encoder: Encoder | null;
  decoder: Decoder | null;
}

let codecs: Codecs | null = null;

/** Finds the platform's codecs once, and keeps only those that behave as the pure ones do. */
function platformCodecs(): Codecs {
  if (codecs) return codecs;
  const host = globalThis as unknown as TextCodecHost;
  let encoder: Encoder | null = null;
  let decoder: Decoder | null = null;
  try {
    if (typeof host.TextEncoder === "function") {
      const candidate = new host.TextEncoder();
      const probe = candidate.encode("aé€😀");
      if (probe instanceof Uint8Array && bytesToHex(probe) === "61c3a9e282acf09f9880") encoder = candidate;
    }
  } catch {
    encoder = null;
  }
  try {
    if (typeof host.TextDecoder === "function") {
      const candidate = new host.TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
      let strict = false;
      try {
        candidate.decode(Uint8Array.of(0xff));
      } catch {
        strict = true;
      }
      const keepsMark = candidate.decode(Uint8Array.of(0xef, 0xbb, 0xbf, 0x61)) === "﻿a";
      if (strict && keepsMark) decoder = candidate;
    }
  } catch {
    decoder = null;
  }
  codecs = { encoder, decoder };
  return codecs;
}

/** UTF-8 bytes of `text`. */
export function utf8Encode(text: string): Uint8Array {
  const { encoder } = platformCodecs();
  return encoder ? encoder.encode(text) : utf8EncodePure(text);
}

/** The text of strictly valid UTF-8 `bytes`. Throws on anything else. */
export function utf8Decode(bytes: Uint8Array): string {
  const { decoder } = platformCodecs();
  if (!decoder) return utf8DecodePure(bytes);
  try {
    return decoder.decode(bytes);
  } catch {
    throw new Error(INVALID_UTF8);
  }
}
