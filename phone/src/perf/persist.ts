import type { SyncStore } from '@/platform';

import type { FrameStats } from './frames';
import { perf, startup } from './marks';

/** What the app leaves in its storage for the scenario run to read after a launch. */
export type PerfStored = {
  /** The measurements of this launch, as JSON. */
  last: string;
  /** The busy-logic test's frames, as JSON, when it was run. */
  busy: string;
  /** How many times the app has started: tells one launch's record from the next. */
  launches: number;
  /** The frames of every scroll of a measured list since the app started, by list, as JSON. */
  scroll: string;
};

/** The frames of one list's scrolls, added up. */
export interface ScrollFrames {
  readonly scrolls: number;
  readonly frames: number;
  readonly dropped: number;
  readonly droppedPercent: number;
  readonly longest: number;
  readonly seconds: number;
}

const scrolled = new Map<string, ScrollFrames>();

/** Adds one scroll's frames to its list's total and writes every list's total. */
export function persistScroll(store: SyncStore<PerfStored>, list: string, drawn: FrameStats): ScrollFrames {
  const was = scrolled.get(list) ?? { scrolls: 0, frames: 0, dropped: 0, droppedPercent: 0, longest: 0, seconds: 0 };
  const frames = was.frames + drawn.frames;
  const dropped = was.dropped + drawn.dropped;
  const now: ScrollFrames = {
    scrolls: was.scrolls + 1,
    frames,
    dropped,
    droppedPercent: frames + dropped === 0 ? 0 : Math.round((dropped / (frames + dropped)) * 1000) / 10,
    longest: Math.max(was.longest, drawn.longest),
    seconds: Math.round((was.seconds + drawn.seconds) * 10) / 10,
  };
  scrolled.set(list, now);
  try {
    store.set('scroll', JSON.stringify(Object.fromEntries(scrolled)));
  } catch {
    // Measuring never breaks the app.
  }
  return now;
}

let counted = false;

/** Writes this launch's measurements. Cheap enough to call at each of the few moments that matter. */
export function persistPerf(store: SyncStore<PerfStored>): void {
  try {
    if (!counted) {
      counted = true;
      store.set('launches', (store.get('launches') ?? 0) + 1);
    }
    const report = perf.report();
    store.set('last', JSON.stringify({ launch: store.get('launches') ?? 0, startup: startup(), marks: report.marks, summary: report.summary }));
  } catch {
    // Measuring never breaks the app.
  }
}

export function persistBusy(store: SyncStore<PerfStored>, frames: FrameStats): void {
  try {
    store.set('busy', JSON.stringify(frames));
  } catch {
    // Measuring never breaks the app.
  }
}
