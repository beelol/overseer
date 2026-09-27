import { frameStats, Perf, percentile } from '@/perf';

describe('what the app measures about itself', () => {
  test('a moment is kept once, in milliseconds since the process started', () => {
    let now = 1_500;
    const perf = new Perf(() => now, () => 1_000);
    perf.mark('screen.agents');
    now = 9_000;
    perf.mark('screen.agents');
    expect(perf.report().marks).toEqual({ 'screen.agents': 500 });
  });

  test('begin and end time one thing; an end without a begin is nothing', () => {
    let now = 0;
    const perf = new Perf(() => now, () => 0);
    perf.end('send');
    perf.begin('send');
    now = 32;
    perf.end('send');
    perf.end('send');
    expect(perf.report().measures.map((m) => [m.name, m.ms])).toEqual([['send', 32]]);
  });

  test('the summary gives the median, the 95th percentile and the longest', () => {
    const perf = new Perf(() => 0, () => 0);
    for (let i = 1; i <= 20; i++) perf.record('tap', i);
    expect(perf.report().summary['tap']).toEqual({ count: 20, p50: 10, p95: 19, max: 20 });
    expect(percentile([], 0.95)).toBe(0);
    expect(percentile([7], 0.95)).toBe(7);
  });

  test('dropped frames are a share of the frames the display had room for', () => {
    expect(frameStats(99, 1, 33.4, 1_670, 16.7)).toEqual({ frames: 99, dropped: 1, droppedPercent: 1, longest: 33.4, period: 16.7, seconds: 1.7 });
    expect(frameStats(0, 0, 0, 0, 16.7).droppedPercent).toBe(0);
  });
});
