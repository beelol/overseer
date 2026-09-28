import type { SyncStore } from '@/platform';

/**
 * What the scenario run may set, through `overseer://test`. Each can only take something away
 * or slow the app down: none gives access to anything.
 */
export type TestSettings = {
  /** `off` starts the app with no door: for measuring that the door makes nothing slower. */
  door: 'on' | 'off';
  /** Milliseconds the app's logic is held at every start: the seeded slow change the run must catch. */
  slow: number;
};

/** The longest a test may hold the start. */
export const SLOW_LIMIT_MS = 5_000;

let started = false;

/**
 * True the first time it is asked in a process, false ever after: the door belongs to a cold
 * start. Coming back from the background keeps the process, so it shows no door.
 */
export function coldStart(): boolean {
  if (started) return false;
  started = true;
  return true;
}

export function doorEnabled(settings: SyncStore<TestSettings>): boolean {
  try {
    return settings.get('door') !== 'off';
  } catch {
    return true;
  }
}

/** Holds the app's logic for as long as a test asked, and says for how long. */
export function seededSlowness(settings: SyncStore<TestSettings>): number {
  let ms = 0;
  try {
    const asked = settings.get('slow');
    if (typeof asked === 'number' && asked > 0) ms = Math.min(asked, SLOW_LIMIT_MS);
  } catch {
    return 0;
  }
  const end = Date.now() + ms;
  let n = 0;
  while (Date.now() < end) n += Math.sqrt(n + 1);
  return ms;
}
