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
};

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
