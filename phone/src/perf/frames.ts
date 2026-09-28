import { useCallback } from 'react';
import { useFrameCallback, useSharedValue, type SharedValue } from 'react-native-reanimated';

export interface FrameStats {
  /** Frames drawn while the monitor ran. */
  readonly frames: number;
  /** Frames the display showed twice because the next one was late. */
  readonly dropped: number;
  /** Dropped frames as a share of all frames the display had room for, in percent. */
  readonly droppedPercent: number;
  /** The longest time between two frames, in milliseconds. */
  readonly longest: number;
  /** The display's frame time the monitor measured against, in milliseconds. */
  readonly period: number;
  readonly seconds: number;
}

/** What a run of frame times says. `period` is the display's frame time (16.7 ms at 60 Hz). */
export function frameStats(frames: number, dropped: number, longest: number, total: number, period: number): FrameStats {
  const slots = frames + dropped;
  return {
    frames,
    dropped,
    droppedPercent: slots === 0 ? 0 : Math.round((dropped / slots) * 1000) / 10,
    longest: Math.round(longest * 10) / 10,
    period: Math.round(period * 10) / 10,
    seconds: Math.round(total / 100) / 10,
  };
}

/** A frame that came late: when, in milliseconds after the monitor's first frame, and how late. */
export interface Stall {
  readonly at: number;
  readonly ms: number;
}

/** The most stalls one run keeps: enough to see where an opening lost its frames. */
const STALLS = 8;

/**
 * Counts frames on the UI thread, where animations run: a frame that comes later than one and
 * a half frame times after the last counts as dropped, once for every frame time it missed.
 * It keeps counting while the app's logic is busy, which is what it is there to show.
 *
 * `counting` (given) may be set on the UI thread the moment what is measured starts and ends (the door's
 * first and last frame): frames outside it belong to something else, however early `start` or
 * late `stop` is called. `start({ counting: false })` waits for it to be set.
 */
export function useFrameMonitor(given?: SharedValue<boolean>): { start(options?: { counting?: boolean }): void; stop(): FrameStats; stalls(): readonly Stall[]; span(): { first: number; last: number } } {
  const frames = useSharedValue(0);
  const dropped = useSharedValue(0);
  const longest = useSharedValue(0);
  const total = useSharedValue(0);
  const period = useSharedValue(0);
  const late = useSharedValue<Stall[]>([]);
  const own = useSharedValue(false);
  const counting = given ?? own;
  // The first and the last frame counted, on the UI thread's clock (milliseconds).
  const first = useSharedValue(0);
  const last = useSharedValue(0);

  const callback = useFrameCallback((info) => {
    'worklet';
    const between = info.timeSincePreviousFrame;
    if (!counting.value) return;
    if (first.value === 0) first.value = info.timestamp;
    last.value = info.timestamp;
    if (between === null) return;
    frames.value += 1;
    total.value += between;
    // The display's own frame time: the shortest seen, no faster than 120 Hz.
    if (period.value === 0 || (between < period.value && between > 8)) period.value = between;
    if (between > longest.value) longest.value = between;
    if (period.value > 0 && between > period.value * 1.5) {
      dropped.value += Math.round(between / period.value) - 1;
      // Where the frame before this one should have come.
      if (late.value.length < STALLS) late.value = [...late.value, { at: Math.round(total.value - between), ms: Math.round(between) }];
    }
  }, false);

  const start = useCallback((options?: { counting?: boolean }) => {
    frames.set(0);
    dropped.set(0);
    longest.set(0);
    total.set(0);
    late.set([]);
    first.set(0);
    last.set(0);
    counting.set(options?.counting ?? true);
    callback.setActive(true);
  }, [callback, frames, dropped, longest, total, late, counting, first, last]);

  const stop = useCallback(() => {
    callback.setActive(false);
    return frameStats(frames.get(), dropped.get(), longest.get(), total.get(), period.get() || 1000 / 60);
  }, [callback, frames, dropped, longest, total, period]);

  const stalls = useCallback((): readonly Stall[] => late.get(), [late]);

  const span = useCallback(() => ({ first: first.get(), last: last.get() }), [first, last]);

  return { start, stop, stalls, span };
}
