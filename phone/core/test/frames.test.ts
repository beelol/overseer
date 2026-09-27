import { describe, expect, it } from "vitest";
import { CHUNK_SIZE, FLAG_LAST, FLAG_MORE, FrameError, Joiner, MAX_REPLY_BYTES, MAX_REQUEST_BYTES, Opener, seal, splitChunks } from "../src/frames.ts";
import { CipherState, NoiseError, TAG_LENGTH } from "../src/noise.ts";

const key = new Uint8Array(32).map((_, i) => i * 3 + 1);

function ciphers() {
  return { tx: new CipherState(key), rx: new CipherState(key) };
}

function pattern(length: number): Uint8Array {
  const out = new Uint8Array(length);
  for (let i = 0; i < length; i++) out[i] = i % 251;
  return out;
}

function roundTrip(length: number, limit = MAX_REQUEST_BYTES) {
  const { tx, rx } = ciphers();
  const message = pattern(length);
  const frames = seal(tx, message);
  const opener = new Opener(limit);
  const results = frames.map((frame) => opener.open(rx, frame));
  return { message, frames, results, joined: results[results.length - 1] };
}

describe("chunking", () => {
  it("matches the limits of the specification", () => {
    expect(CHUNK_SIZE).toBe(65_000);
    expect(MAX_REQUEST_BYTES).toBe(1_048_576);
    expect(MAX_REPLY_BYTES).toBe(67_108_864);
    expect([FLAG_MORE, FLAG_LAST]).toEqual([0, 1]);
  });

  it("an empty message is one frame that holds only the flag", () => {
    expect(splitChunks(new Uint8Array(0))).toEqual([Uint8Array.of(FLAG_LAST)]);
    const { frames, joined } = roundTrip(0);
    expect(frames.length).toBe(1);
    expect((frames[0] as Uint8Array).length).toBe(1 + TAG_LENGTH);
    expect(joined).toEqual(new Uint8Array(0));
  });

  it("exactly 65,000 bytes is one frame", () => {
    const { message, frames, joined } = roundTrip(65_000);
    expect(frames.map((f) => f.length)).toEqual([1 + 65_000 + TAG_LENGTH]);
    expect(joined).toEqual(message);
  });

  it("65,001 bytes is two frames, the second with one byte", () => {
    const { message, frames, results } = roundTrip(65_001);
    expect(frames.map((f) => f.length)).toEqual([1 + 65_000 + TAG_LENGTH, 1 + 1 + TAG_LENGTH]);
    expect(results[0]).toBeNull();
    expect(results[1]).toEqual(message);
    expect(splitChunks(message).map((c) => c[0])).toEqual([FLAG_MORE, FLAG_LAST]);
  });

  it("200,000 bytes is four frames, as in the daemon's test", () => {
    const { message, frames, results } = roundTrip(200_000);
    expect(frames.length).toBe(4);
    expect(frames.map((f) => f.length - 1 - TAG_LENGTH)).toEqual([65_000, 65_000, 65_000, 5_000]);
    expect(results.slice(0, 3)).toEqual([null, null, null]);
    expect(results[3]).toEqual(message);
  });

  it("every frame fits a Noise message", () => {
    const { frames } = roundTrip(130_000);
    for (const frame of frames) expect(frame.length).toBeLessThanOrEqual(65_535);
  });

  it("joins several messages in a row with one opener", () => {
    const { tx, rx } = ciphers();
    const opener = new Opener(MAX_REQUEST_BYTES);
    for (const length of [0, 5, 65_000, 70_000, 1]) {
      let joined: Uint8Array | null = null;
      for (const frame of seal(tx, pattern(length))) {
        expect(joined).toBeNull();
        joined = opener.open(rx, frame);
      }
      expect(joined).toEqual(pattern(length));
      expect(opener.pending).toBe(false);
    }
  });

  it("does not hand out memory it keeps using", () => {
    const joiner = new Joiner(100);
    const chunk = Uint8Array.of(FLAG_LAST, 1, 2, 3);
    const message = joiner.push(chunk) as Uint8Array;
    chunk[1] = 9;
    expect(message).toEqual(Uint8Array.of(1, 2, 3));
  });
});

describe("limits", () => {
  it("a message at the limit passes and one byte more is refused", () => {
    expect(roundTrip(64, 64).joined).toEqual(pattern(64));
    const { tx, rx } = ciphers();
    const [frame] = seal(tx, pattern(65)) as [Uint8Array];
    expect(() => new Opener(64).open(rx, frame)).toThrowError(FrameError);
    expect(() => new Opener(64).open(new CipherState(key), frame)).toThrowError(/larger than the limit/);
  });

  it("a message over the limit is refused at the chunk that crosses it, before the last one", () => {
    const { tx, rx } = ciphers();
    const frames = seal(tx, pattern(200_000));
    const opener = new Opener(100_000);
    expect(opener.open(rx, frames[0] as Uint8Array)).toBeNull();
    try {
      opener.open(rx, frames[1] as Uint8Array);
      expect.unreachable();
    } catch (error) {
      expect(error).toBeInstanceOf(FrameError);
      expect((error as FrameError).code).toBe("message_too_large");
    }
  });

  it("a request of exactly 1 MiB passes the request limit", () => {
    const { message, frames, joined } = roundTrip(MAX_REQUEST_BYTES);
    expect(frames.length).toBe(17);
    expect(joined).toEqual(message);
  });
});

describe("malformed frames", () => {
  function sealRaw(plain: Uint8Array): { frame: Uint8Array; rx: CipherState } {
    const { tx, rx } = ciphers();
    return { frame: tx.encrypt(plain), rx };
  }

  it("a flag that is neither more nor last is refused", () => {
    for (const flag of [0x02, 0x7f, 0x80, 0xff]) {
      const { frame, rx } = sealRaw(Uint8Array.of(flag, 1, 2, 3));
      try {
        new Opener(64).open(rx, frame);
        expect.unreachable();
      } catch (error) {
        expect(error).toBeInstanceOf(FrameError);
        expect((error as FrameError).code).toBe("bad_flag");
      }
    }
  });

  it("a chunk larger than 65,000 bytes is refused", () => {
    const plain = new Uint8Array(1 + 65_001);
    plain[0] = FLAG_LAST;
    const { frame, rx } = sealRaw(plain);
    try {
      new Opener(MAX_REQUEST_BYTES).open(rx, frame);
      expect.unreachable();
    } catch (error) {
      expect((error as FrameError).code).toBe("chunk_too_large");
    }
  });

  it("a frame without even a flag is refused", () => {
    const { frame, rx } = sealRaw(new Uint8Array(0));
    expect(frame.length).toBe(TAG_LENGTH);
    expect(() => new Opener(64).open(rx, frame)).toThrowError(FrameError);
    expect(() => new Joiner(64).push(new Uint8Array(0))).toThrowError(FrameError);
  });

  it("a frame longer than a Noise message is refused before it is decrypted", () => {
    const rx = new CipherState(key);
    expect(() => new Opener(MAX_REQUEST_BYTES).open(rx, new Uint8Array(65_536))).toThrowError(FrameError);
    expect(rx.nonce).toEqual({ high: 0, low: 0 });
  });

  it("a frame that was changed is refused by the cipher", () => {
    const { tx, rx } = ciphers();
    const [frame] = seal(tx, pattern(10)) as [Uint8Array];
    frame[3] = (frame[3] as number) ^ 1;
    expect(() => new Opener(64).open(rx, frame)).toThrowError(NoiseError);
  });

  it("a missing chunk in the middle of a message is noticed", () => {
    const { tx, rx } = ciphers();
    const frames = seal(tx, pattern(200_000));
    const opener = new Opener(MAX_REQUEST_BYTES);
    opener.open(rx, frames[0] as Uint8Array);
    expect(() => opener.open(rx, frames[2] as Uint8Array)).toThrowError(NoiseError);
  });
});
