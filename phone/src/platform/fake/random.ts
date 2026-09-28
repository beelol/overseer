import { defineCapability, type Support } from '../capability';
import { checkByteCount, uuidFromBytes, type RandomCapability } from '../capabilities/random';
import { createFakeSupport, type FakeSupport } from './support';

export interface FakeRandom {
  readonly capability: RandomCapability;
  readonly support: FakeSupport;
}

/**
 * Bytes from a small seeded generator (mulberry32): the same seed gives the same bytes in every
 * run, which is what a test wants and exactly what real key material must never be.
 */
export function createFakeRandom(seed = 1, initial?: Support): FakeRandom {
  const support = createFakeSupport('random', initial);
  let state = seed >>> 0;
  function next(): number {
    state = (state + 0x6d2b79f5) >>> 0;
    let mixed = Math.imul(state ^ (state >>> 15), state | 1);
    mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61);
    return (mixed ^ (mixed >>> 14)) >>> 0;
  }
  function bytes(length: number): Uint8Array {
    support.require();
    checkByteCount(length);
    return Uint8Array.from({ length }, () => next() & 0xff);
  }
  const capability = defineCapability<RandomCapability>('random', support.check, {
    bytes,
    uuid: () => uuidFromBytes(bytes(16)),
  });
  return { capability, support };
}
