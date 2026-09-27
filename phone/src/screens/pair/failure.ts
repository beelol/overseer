import { OverseerError, PairingError } from '@/core';

import { PAIR } from './words';

/** Outcomes of an address at which nothing answered. Any other outcome is an answer of a Mac. */
const SILENT: ReadonlySet<string> = new Set(['unreachable', 'timeout', 'cancelled']);

/**
 * What to say when pairing failed: one of the brief's two sentences. A Mac that answered and
 * refused means the code is wrong, used or too old; silence at every address means the Mac was
 * not reached.
 */
export function pairingSentence(error: unknown): string {
  if (error instanceof PairingError) {
    return error.attempts.some((attempt) => !SILENT.has(attempt.outcome))
      ? PAIR.codeDidNotWork
      : PAIR.couldNotReach;
  }
  if (error instanceof OverseerError && SILENT.has(error.code)) return PAIR.couldNotReach;
  return PAIR.codeDidNotWork;
}
