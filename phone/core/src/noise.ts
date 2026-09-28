/**
 * The Noise framework (revision 34) for the two patterns the gateway uses:
 * `Noise_IK_25519_ChaChaPoly_SHA256` for sessions and `Noise_IKpsk1_25519_ChaChaPoly_SHA256`
 * for pairing, both with the prologue `overseer-gateway-v1`.
 *
 * Pairing is `psk1`, not `psk2`: the pairing secret is mixed in at the end of the first message,
 * so the gateway cannot read message 1 from a phone that does not hold the pairing code.
 *
 * The output is byte for byte what the daemon's `snow` produces; `protocol/vectors/noise.json`
 * holds the shared vectors and `test/noise.test.ts` checks every value in it.
 */

import { chacha20poly1305 } from "@noble/ciphers/chacha.js";
import { x25519 } from "@noble/curves/ed25519.js";
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex, concatBytes, copyBytes, utf8Encode, wipe } from "./bytes.ts";
import { OverseerError } from "./errors.ts";
import type { RandomSource } from "./platform.ts";

export const PROLOGUE = "overseer-gateway-v1";
export const PATTERN_SESSION = "Noise_IK_25519_ChaChaPoly_SHA256";
export const PATTERN_PAIRING = "Noise_IKpsk1_25519_ChaChaPoly_SHA256";

/** Bytes in a key, a public key and a hash. */
export const KEY_LENGTH = 32;
/** Bytes the authentication tag adds to every encrypted message. */
export const TAG_LENGTH = 16;
/** The largest Noise message. */
export const MAX_NOISE_MESSAGE = 65_535;

/** Which handshake: a session of a paired device, or the pairing itself. */
export type HandshakeKind = "session" | "pairing";
export type HandshakeRole = "initiator" | "responder";

/** An X25519 key pair. */
export interface KeyPair {
  readonly privateKey: Uint8Array;
  readonly publicKey: Uint8Array;
}

/** A failure inside Noise. The message never says more than which step failed. */
export class NoiseError extends OverseerError {
  constructor(code: "decrypt" | "length" | "state" | "nonce_exhausted" | "bad_key", message: string) {
    super(code, message);
    this.name = "NoiseError";
  }
}

const EMPTY = new Uint8Array(0);
const UINT32_MAX = 0xffff_ffff;

function requireKey(key: Uint8Array, what: string): void {
  if (!(key instanceof Uint8Array) || key.length !== KEY_LENGTH) {
    throw new NoiseError("bad_key", `${what} must be ${KEY_LENGTH} bytes`);
  }
}

/** The public key of an X25519 private key. */
export function publicKeyOf(privateKey: Uint8Array): Uint8Array {
  requireKey(privateKey, "a private key");
  return x25519.getPublicKey(privateKey);
}

/** Creates a key pair from 32 bytes of the injected random source. */
export function generateKeyPair(random: RandomSource): KeyPair {
  const privateKey = copyBytes(random(KEY_LENGTH));
  if (privateKey.length !== KEY_LENGTH) throw new NoiseError("bad_key", "the random source returned the wrong number of bytes");
  if (privateKey.every((b) => b === 0)) throw new NoiseError("bad_key", "the random source returned only zeros");
  return { privateKey, publicKey: x25519.getPublicKey(privateKey) };
}

/** A key's fingerprint: the first 16 hex characters of SHA-256 over the public key. */
export function fingerprint(publicKey: Uint8Array): string {
  return bytesToHex(sha256(publicKey)).slice(0, 16);
}

/** The pre-shared key of a pairing handshake: SHA-256 of the pairing secret. */
export function pskFromSecret(secret: Uint8Array): Uint8Array {
  return sha256(secret);
}

function dh(privateKey: Uint8Array, publicKey: Uint8Array): Uint8Array {
  try {
    return x25519.getSharedSecret(privateKey, publicKey);
  } catch {
    // A public key of low order gives an all-zero secret; it is refused.
    throw new NoiseError("bad_key", "the key exchange was refused");
  }
}

function hkdf(chainingKey: Uint8Array, material: Uint8Array, outputs: 2 | 3): Uint8Array[] {
  const temp = hmac(sha256, chainingKey, material);
  const one = hmac(sha256, temp, Uint8Array.of(1));
  const two = hmac(sha256, temp, concatBytes(one, Uint8Array.of(2)));
  const out = [one, two];
  if (outputs === 3) out.push(hmac(sha256, temp, concatBytes(two, Uint8Array.of(3))));
  wipe(temp);
  return out;
}

/** Where a cipher's nonce counter starts. Only tests start anywhere but zero. */
export interface NonceStart {
  /** The high 32 bits of the 64-bit counter. */
  readonly high: number;
  /** The low 32 bits of the 64-bit counter. */
  readonly low: number;
}

/**
 * One direction of an encrypted channel: a ChaCha20-Poly1305 key and a 64-bit nonce counter
 * that starts at zero and goes up by one for every message. A nonce is never used twice: the
 * counter moves only forward, and the last value (2^64 - 1) is refused as Noise requires.
 */
export class CipherState {
  private key: Uint8Array | null;
  private high: number;
  private low: number;

  /** `start` exists for tests of the counter's end; everything else starts at zero. */
  constructor(key: Uint8Array, start?: NonceStart) {
    requireKey(key, "a cipher key");
    this.key = copyBytes(key);
    this.high = start ? start.high >>> 0 : 0;
    this.low = start ? start.low >>> 0 : 0;
  }

  /** How many messages this cipher has processed, as the two halves of the counter. */
  get nonce(): NonceStart {
    return { high: this.high, low: this.low };
  }

  private material(): { key: Uint8Array; nonce: Uint8Array } {
    if (!this.key) throw new NoiseError("state", "this cipher was destroyed");
    if (this.high === UINT32_MAX && this.low === UINT32_MAX) {
      throw new NoiseError("nonce_exhausted", "this key has protected as many messages as it may");
    }
    // Noise's ChaChaPoly nonce: 32 zero bits, then the counter as 64 bits, little endian.
    const nonce = new Uint8Array(12);
    const view = new DataView(nonce.buffer);
    view.setUint32(4, this.low, true);
    view.setUint32(8, this.high, true);
    return { key: this.key, nonce };
  }

  private advance(): void {
    if (this.low === UINT32_MAX) {
      this.low = 0;
      this.high += 1;
    } else {
      this.low += 1;
    }
  }

  /** Encrypts `plaintext`, bound to `associated`. The result is 16 bytes longer. */
  encrypt(plaintext: Uint8Array, associated: Uint8Array = EMPTY): Uint8Array {
    if (plaintext.length > MAX_NOISE_MESSAGE - TAG_LENGTH) throw new NoiseError("length", "the message is too long for one Noise message");
    const { key, nonce } = this.material();
    const out = chacha20poly1305(key, nonce, associated).encrypt(plaintext);
    this.advance();
    return out;
  }

  /**
   * Decrypts and authenticates `ciphertext`. Throws when it was changed, replayed, reordered or
   * cut short. The counter moves only when the message was genuine.
   */
  decrypt(ciphertext: Uint8Array, associated: Uint8Array = EMPTY): Uint8Array {
    if (ciphertext.length < TAG_LENGTH || ciphertext.length > MAX_NOISE_MESSAGE) throw new NoiseError("length", "the message has an impossible length");
    const { key, nonce } = this.material();
    let out: Uint8Array;
    try {
      out = chacha20poly1305(key, nonce, associated).decrypt(ciphertext);
    } catch {
      throw new NoiseError("decrypt", "the message did not decrypt");
    }
    this.advance();
    return out;
  }

  /** Forgets the key. Every later use throws. */
  destroy(): void {
    wipe(this.key);
    this.key = null;
  }
}

type Token = "e" | "s" | "ee" | "es" | "se" | "ss" | "psk";

/**
 * The tokens of each handshake message. Everything the handshake does follows from these
 * tables: a `psk` token is MixKeyAndHash, and in a pattern that has one, every `e` token also
 * mixes the ephemeral public key into the key (Noise, section 9.2).
 */
const MESSAGES: Readonly<Record<HandshakeKind, readonly (readonly Token[])[]>> = {
  // IK
  session: [
    ["e", "es", "s", "ss"],
    ["e", "ee", "se"],
  ],
  // IKpsk1: the pre-shared key ends the first message.
  pairing: [
    ["e", "es", "s", "ss", "psk"],
    ["e", "ee", "se"],
  ],
};

function usesPsk(kind: HandshakeKind): boolean {
  return MESSAGES[kind].some((tokens) => tokens.includes("psk"));
}

const PATTERNS: Readonly<Record<HandshakeKind, string>> = {
  session: PATTERN_SESSION,
  pairing: PATTERN_PAIRING,
};

/** What a handshake needs. */
export interface HandshakeOptions {
  readonly kind: HandshakeKind;
  readonly role: HandshakeRole;
  /** This side's static private key. */
  readonly staticPrivateKey: Uint8Array;
  /**
   * This side's static public key, when it is known already. It saves one scalar
   * multiplication per handshake. A key that does not belong to the private key makes the
   * handshake fail at the other side; it cannot weaken it.
   */
  readonly staticPublicKey?: Uint8Array;
  /** The gateway's static public key. Required for the initiator; the responder learns the device's. */
  readonly remoteStaticPublicKey?: Uint8Array;
  /** The pre-shared key, `pskFromSecret(pairing secret)`. Required for pairing. */
  readonly psk?: Uint8Array;
  /** Source of the ephemeral key. */
  readonly random: RandomSource;
  /** FOR TESTS ONLY: a fixed ephemeral private key, to reproduce the shared vectors. */
  readonly ephemeralPrivateKey?: Uint8Array;
}

/** The result of a finished handshake. */
export interface TransportKeys {
  /** Encrypts what this side sends. Its nonce starts at zero. */
  readonly send: CipherState;
  /** Decrypts what this side receives. Its nonce starts at zero. */
  readonly receive: CipherState;
  /** The handshake hash: the same on both sides, unique to this session. */
  readonly handshakeHash: Uint8Array;
  /** The static public key the other side proved it holds. */
  readonly remoteStaticPublicKey: Uint8Array;
}

/**
 * One handshake, for either role. The initiator calls `writeMessage`, then `readMessage`; the
 * responder the reverse; then both call `split`. After any failure the handshake is unusable.
 */
export class Handshake {
  readonly kind: HandshakeKind;
  readonly role: HandshakeRole;
  private h: Uint8Array;
  private ck: Uint8Array;
  private cipher: CipherState | null = null;
  private readonly s: KeyPair;
  private e: KeyPair | null = null;
  private rs: Uint8Array | null;
  private re: Uint8Array | null = null;
  private readonly psk: Uint8Array | null;
  private readonly random: RandomSource;
  private readonly fixedEphemeral: Uint8Array | null;
  private step = 0;
  private broken = false;
  private finished = false;

  constructor(options: HandshakeOptions) {
    this.kind = options.kind;
    this.role = options.role;
    this.random = options.random;
    requireKey(options.staticPrivateKey, "the static private key");
    const privateKey = copyBytes(options.staticPrivateKey);
    if (options.staticPublicKey) requireKey(options.staticPublicKey, "the static public key");
    this.s = { privateKey, publicKey: options.staticPublicKey ? copyBytes(options.staticPublicKey) : x25519.getPublicKey(privateKey) };
    if (options.role === "initiator") {
      if (!options.remoteStaticPublicKey) throw new NoiseError("bad_key", "the initiator needs the gateway's public key");
      requireKey(options.remoteStaticPublicKey, "the gateway's public key");
      this.rs = copyBytes(options.remoteStaticPublicKey);
    } else {
      this.rs = null;
    }
    if (usesPsk(options.kind)) {
      if (!options.psk) throw new NoiseError("bad_key", "pairing needs the pre-shared key");
      requireKey(options.psk, "the pre-shared key");
      this.psk = copyBytes(options.psk);
    } else {
      if (options.psk) throw new NoiseError("bad_key", "a session takes no pre-shared key");
      this.psk = null;
    }
    if (options.ephemeralPrivateKey) {
      requireKey(options.ephemeralPrivateKey, "the ephemeral private key");
      this.fixedEphemeral = copyBytes(options.ephemeralPrivateKey);
    } else {
      this.fixedEphemeral = null;
    }

    const name = utf8Encode(PATTERNS[options.kind]);
    if (name.length <= KEY_LENGTH) {
      this.h = new Uint8Array(KEY_LENGTH);
      this.h.set(name);
    } else {
      this.h = sha256(name);
    }
    this.ck = copyBytes(this.h);
    this.mixHash(utf8Encode(PROLOGUE));
    // IK's pre-message: the responder's static key is known to both sides.
    this.mixHash(options.role === "initiator" ? (this.rs as Uint8Array) : this.s.publicKey);
  }

  /** True when both handshake messages were processed. */
  get complete(): boolean {
    return this.step === 2 && !this.broken;
  }

  /** The handshake hash so far; final once `complete`. */
  get handshakeHash(): Uint8Array {
    return copyBytes(this.h);
  }

  /** The other side's static public key, once known (the responder learns it from message 1). */
  get remoteStaticPublicKey(): Uint8Array | null {
    return this.rs ? copyBytes(this.rs) : null;
  }

  private mixHash(data: Uint8Array): void {
    this.h = sha256(concatBytes(this.h, data));
  }

  private setKey(key: Uint8Array): void {
    this.cipher?.destroy();
    this.cipher = new CipherState(key);
    wipe(key);
  }

  private mixKey(material: Uint8Array): void {
    const [ck, key] = hkdf(this.ck, material, 2) as [Uint8Array, Uint8Array];
    wipe(this.ck);
    this.ck = ck;
    this.setKey(key);
  }

  private mixKeyAndHash(material: Uint8Array): void {
    const [ck, hash, key] = hkdf(this.ck, material, 3) as [Uint8Array, Uint8Array, Uint8Array];
    wipe(this.ck);
    this.ck = ck;
    this.mixHash(hash);
    this.setKey(key);
  }

  private encryptAndHash(plaintext: Uint8Array): Uint8Array {
    const out = this.cipher ? this.cipher.encrypt(plaintext, this.h) : copyBytes(plaintext);
    this.mixHash(out);
    return out;
  }

  private decryptAndHash(ciphertext: Uint8Array): Uint8Array {
    const out = this.cipher ? this.cipher.decrypt(ciphertext, this.h) : copyBytes(ciphertext);
    this.mixHash(ciphertext);
    return out;
  }

  private mixDh(token: "ee" | "es" | "se" | "ss"): void {
    const initiator = this.role === "initiator";
    // The first letter names the initiator's key, the second the responder's.
    const mineIsEphemeral = initiator ? token[0] === "e" : token[1] === "e";
    const theirsIsEphemeral = initiator ? token[1] === "e" : token[0] === "e";
    const mine = mineIsEphemeral ? this.e?.privateKey : this.s.privateKey;
    const theirs = theirsIsEphemeral ? this.re : this.rs;
    if (!mine || !theirs) throw new NoiseError("state", "the handshake is missing a key");
    const secret = dh(mine, theirs);
    this.mixKey(secret);
    wipe(secret);
  }

  private begin(writing: boolean): readonly Token[] {
    if (this.broken) throw new NoiseError("state", "this handshake failed and cannot be used");
    if (this.finished || this.step >= 2) throw new NoiseError("state", "this handshake is finished");
    const myTurn = (this.step === 0) === (this.role === "initiator");
    if (writing !== myTurn) throw new NoiseError("state", "the handshake messages are out of order");
    return (MESSAGES[this.kind] as readonly (readonly Token[])[])[this.step] as readonly Token[];
  }

  /** Writes the next handshake message with `payload` inside. */
  writeMessage(payload: Uint8Array = EMPTY): Uint8Array {
    const tokens = this.begin(true);
    try {
      const parts: Uint8Array[] = [];
      for (const token of tokens) {
        if (token === "e") {
          const privateKey = this.fixedEphemeral ?? generateKeyPair(this.random).privateKey;
          this.e = { privateKey, publicKey: x25519.getPublicKey(privateKey) };
          parts.push(this.e.publicKey);
          this.mixHash(this.e.publicKey);
          if (this.psk) this.mixKey(copyBytes(this.e.publicKey));
        } else if (token === "s") {
          parts.push(this.encryptAndHash(this.s.publicKey));
        } else if (token === "psk") {
          this.mixKeyAndHash(this.psk as Uint8Array);
        } else {
          this.mixDh(token);
        }
      }
      parts.push(this.encryptAndHash(payload));
      const message = concatBytes(...parts);
      if (message.length > MAX_NOISE_MESSAGE) throw new NoiseError("length", "the handshake message is too long");
      this.step += 1;
      return message;
    } catch (error) {
      this.broken = true;
      throw error;
    }
  }

  /** Reads the next handshake message and returns its payload. Throws when it is not genuine. */
  readMessage(message: Uint8Array): Uint8Array {
    const tokens = this.begin(false);
    try {
      if (message.length > MAX_NOISE_MESSAGE) throw new NoiseError("length", "the handshake message is too long");
      let offset = 0;
      const take = (length: number): Uint8Array => {
        if (offset + length > message.length) throw new NoiseError("length", "the handshake message is too short");
        const part = message.subarray(offset, offset + length);
        offset += length;
        return part;
      };
      for (const token of tokens) {
        if (token === "e") {
          this.re = copyBytes(take(KEY_LENGTH));
          this.mixHash(this.re);
          if (this.psk) this.mixKey(copyBytes(this.re));
        } else if (token === "s") {
          const length = this.cipher ? KEY_LENGTH + TAG_LENGTH : KEY_LENGTH;
          this.rs = this.decryptAndHash(take(length));
        } else if (token === "psk") {
          this.mixKeyAndHash(this.psk as Uint8Array);
        } else {
          this.mixDh(token);
        }
      }
      const payload = this.decryptAndHash(message.subarray(offset));
      this.step += 1;
      return payload;
    } catch (error) {
      this.broken = true;
      throw error;
    }
  }

  /** Ends a complete handshake and returns the two transport ciphers. Callable once. */
  split(): TransportKeys {
    if (this.broken || this.finished || this.step !== 2 || !this.rs) {
      throw new NoiseError("state", "the handshake is not complete");
    }
    const [first, second] = hkdf(this.ck, EMPTY, 2) as [Uint8Array, Uint8Array];
    const initiator = this.role === "initiator";
    const keys: TransportKeys = {
      send: new CipherState(initiator ? first : second),
      receive: new CipherState(initiator ? second : first),
      handshakeHash: copyBytes(this.h),
      remoteStaticPublicKey: copyBytes(this.rs),
    };
    wipe(first, second);
    this.destroy();
    this.finished = true;
    return keys;
  }

  /** Forgets every secret this handshake holds. */
  destroy(): void {
    this.cipher?.destroy();
    this.cipher = null;
    wipe(this.ck, this.s.privateKey, this.e?.privateKey, this.psk, this.fixedEphemeral);
    this.finished = true;
  }
}
