import { createFakePlatform } from '@/platform/fake';

import { frameStats } from '../frames';
import { persistScroll, type PerfStored } from '../persist';

describe("a list's scrolls, added up for the scenario run", () => {
  test('frames and dropped frames add up per list; the longest frame is the longest of all', () => {
    const store = createFakePlatform().capabilities.keyValue.scope<PerfStored>('perf');
    const period = 1000 / 60;
    persistScroll(store, 'changes', frameStats(60, 0, period, 60 * period, period));
    const after = persistScroll(store, 'changes', frameStats(59, 1, 2 * period, 61 * period, period));
    expect(after).toMatchObject({ scrolls: 2, frames: 119, dropped: 1, droppedPercent: 0.8 });
    expect(after.longest).toBeCloseTo(33.3, 1);
    persistScroll(store, 'agents', frameStats(30, 0, period, 30 * period, period));
    const stored = JSON.parse(store.get('scroll') ?? '{}');
    expect(Object.keys(stored).sort()).toEqual(['agents', 'changes']);
    expect(stored.changes.frames).toBe(119);
  });
});
