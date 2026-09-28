/** The app's measurements of its own speed (AC-135). */
export { frameStats, useFrameMonitor, type FrameStats } from './frames';
export { Perf, perf, percentile, startup, type Measure, type PerfReport } from './marks';
export { persistBusy, persistPerf, persistScroll, type PerfStored, type ScrollFrames } from './persist';
export { useScrollFrames, type ScrollFrameHandlers } from './scrolling';
