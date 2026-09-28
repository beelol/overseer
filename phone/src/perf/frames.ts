import { useCallback } from 'react';
import { useFrameCallback, useSharedValue } from 'react-native-reanimated';

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

/**
 * Counts frames on the UI thread, where animations run: a frame that comes later than one and
 * a half frame times after the last counts as dropped, once for every frame time it missed.
 * It keeps counting while the app's logic is busy, which is what it is there to show.
 */
export function useFrameMonitor(): { start(): void; stop(): FrameStats } {
  const frames = useSharedValue(0);
  const dropped = useSharedValue(0);
  const longest = useSharedValue(0);
  const total = useSharedValue(0);
  const period = useSharedValue(0);

  const callback = useFrameCallback((info) => {
    'worklet';
    const between = info.timeSincePreviousFrame;
    if (between === null) return;
    frames.value += 1;
    total.value += between;
    // The display's own frame time: the shortest seen, no faster than 120 Hz.
    if (period.value === 0 || (between < period.value && between > 8)) period.value = between;
    if (between > longest.value) longest.value = between;
    if (period.value > 0 && between > period.value * 1.5) dropped.value += Math.round(between / period.value) - 1;
  }, false);

  const start = useCallback(() => {
    frames.set(0);
    dropped.set(0);
    longest.set(0);
    total.set(0);
    callback.setActive(true);
  }, [callback, frames, dropped, longest, total]);

  const stop = useCallback(() => {
    callback.setActive(false);
    return frameStats(frames.get(), dropped.get(), longest.get(), total.get(), period.get() || 1000 / 60);
  }, [callback, frames, dropped, longest, total, period]);

  return { start, stop };
}
