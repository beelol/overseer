/** The wait between reconnection passes: doubling from a minimum to a maximum, with jitter. */

import { type RandomSource, randomFraction } from "./platform.ts";

export class Backoff {
  private readonly minMs: number;
  private readonly maxMs: number;
  private readonly random: RandomSource;
  private base: number;

  constructor(minMs: number, maxMs: number, random: RandomSource) {
    this.minMs = minMs;
    this.maxMs = Math.max(minMs, maxMs);
    this.random = random;
    this.base = minMs;
  }

  /** The wait before the next pass, without jitter. */
  get current(): number {
    return this.base;
  }

  /**
   * The next wait. Half of it is fixed and half is random, so it lies between half the current
   * step and the whole step, and phones that lost the Mac together do not return together.
   */
  next(): number {
    const step = this.base;
    this.base = Math.min(this.maxMs, this.base * 2);
    return step / 2 + (step / 2) * randomFraction(this.random);
  }

  /** Back to the minimum, after a success or when the app asks to retry now. */
  reset(): void {
    this.base = this.minMs;
  }
}
