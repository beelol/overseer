/**
 * Transport framing (docs/rfcs/phone-remote-protocol.md, "Transport messages"; the daemon's
 * `seal` and `Opener` in `daemon/src/gateway/noise.rs`).
 *
 * One protocol message becomes one or more chunks. A chunk is a flag byte (`0x00` more chunks
 * follow, `0x01` last chunk) and up to 65,000 bytes; each chunk is one Noise transport message.
 * An empty message is one chunk that holds only the flag.
 */

import { concatBytes } from "./bytes.ts";
import { OverseerError } from "./errors.ts";
import { type CipherState, MAX_NOISE_MESSAGE, TAG_LENGTH } from "./noise.ts";

/** The most message bytes in one chunk. */
export const CHUNK_SIZE = 65_000;
export const FLAG_MORE = 0x00;
export const FLAG_LAST = 0x01;
/** The largest request a device may send, after joining. */
export const MAX_REQUEST_BYTES = 1024 * 1024;
/** The largest reply or event a device accepts, after joining. */
export const MAX_REPLY_BYTES = 64 * 1024 * 1024;

/** A frame or a chunk that breaks the framing rules. The session ends on it. */
export class FrameError extends OverseerError {
  constructor(code: "length" | "bad_flag" | "chunk_too_large" | "message_too_large", message: string) {
    super(code, message);
    this.name = "FrameError";
  }
}

/** Splits a message into plaintext chunks, each with its flag byte in front. */
export function splitChunks(message: Uint8Array): Uint8Array[] {
  if (message.length === 0) return [Uint8Array.of(FLAG_LAST)];
  const chunks: Uint8Array[] = [];
  for (let offset = 0; offset < message.length; offset += CHUNK_SIZE) {
    const end = Math.min(offset + CHUNK_SIZE, message.length);
    const chunk = new Uint8Array(1 + end - offset);
    chunk[0] = end < message.length ? FLAG_MORE : FLAG_LAST;
    chunk.set(message.subarray(offset, end), 1);
    chunks.push(chunk);
  }
  return chunks;
}

/** Joins plaintext chunks into messages. `limit` bounds a joined message. */
export class Joiner {
  private readonly limit: number;
  private parts: Uint8Array[] = [];
  private length = 0;

  constructor(limit: number) {
    this.limit = limit;
  }

  /** True while a message is only partly received. */
  get pending(): boolean {
    return this.parts.length > 0;
  }

  /** Takes one plaintext chunk. Returns the whole message when the chunk was its last. */
  push(chunk: Uint8Array): Uint8Array | null {
    if (chunk.length < 1) throw new FrameError("length", "a chunk without a flag");
    const flag = chunk[0] as number;
    if (flag !== FLAG_MORE && flag !== FLAG_LAST) throw new FrameError("bad_flag", "a chunk with an unknown flag");
    const body = chunk.subarray(1);
    if (body.length > CHUNK_SIZE) throw new FrameError("chunk_too_large", "a chunk larger than 65,000 bytes");
    if (this.length + body.length > this.limit) throw new FrameError("message_too_large", "a message larger than the limit");
    this.parts.push(body);
    this.length += body.length;
    if (flag === FLAG_MORE) return null;
    const message = this.parts.length === 1 ? new Uint8Array(body) : concatBytes(...this.parts);
    this.parts = [];
    this.length = 0;
    return message;
  }
}

/** Splits one protocol message into encrypted transport frames, in the order to send them. */
export function seal(cipher: CipherState, message: Uint8Array): Uint8Array[] {
  return splitChunks(message).map((chunk) => cipher.encrypt(chunk));
}

/** Decrypts transport frames and joins their chunks. Any error means the session must end. */
export class Opener {
  private readonly joiner: Joiner;

  /** `limit` bounds a joined message, in bytes. */
  constructor(limit: number) {
    this.joiner = new Joiner(limit);
  }

  /** True while a message is only partly received. */
  get pending(): boolean {
    return this.joiner.pending;
  }

  /** Returns a whole message when `frame` was its last chunk, otherwise null. */
  open(cipher: CipherState, frame: Uint8Array): Uint8Array | null {
    if (frame.length < 1 + TAG_LENGTH || frame.length > MAX_NOISE_MESSAGE) {
      throw new FrameError("length", "a frame with an impossible length");
    }
    return this.joiner.push(cipher.decrypt(frame));
  }
}
