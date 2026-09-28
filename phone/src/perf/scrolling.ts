import { useCallback, useEffect, useMemo, useRef } from 'react';

import { useCapabilities } from '@/platform';

import { useFrameMonitor } from './frames';
import { perf } from './marks';
import { persistScroll, type PerfStored } from './persist';

/**
 * How long after the finger lifts a fling may still begin, in milliseconds. A scroll that does
 * not fling ends then; one that does ends when it comes to rest.
 */
const FLING_WAIT_MS = 250;

/** What a scroll view is given to have its scrolls' frames counted. */
export interface ScrollFrameHandlers {
  readonly onScrollBeginDrag: () => void;
  readonly onScrollEndDrag: () => void;
  readonly onMomentumScrollBegin: () => void;
  readonly onMomentumScrollEnd: () => void;
}

/**
 * Counts the frames of every scroll of a list on the UI thread, from the finger's first move to
 * the list at rest, and leaves the totals in the app's storage for the scenario run (AC-126,
 * AC-135). `list` names the list in the totals.
 */
export function useScrollFrames(list: string): ScrollFrameHandlers {
  const monitor = useFrameMonitor();
  const { keyValue } = useCapabilities();
  const store = useMemo(() => keyValue.scope<PerfStored>('perf'), [keyValue]);
  const moving = useRef(false);
  const waiting = useRef<ReturnType<typeof setTimeout> | null>(null);

  const wait = useCallback(() => {
    if (waiting.current !== null) clearTimeout(waiting.current);
    waiting.current = null;
  }, []);

  const finish = useCallback(() => {
    wait();
    if (!moving.current) return;
    moving.current = false;
    const drawn = monitor.stop();
    perf.record(`scroll.${list}.dropped`, drawn.dropped);
    persistScroll(store, list, drawn);
  }, [list, monitor, store, wait]);

  useEffect(() => () => wait(), [wait]);

  return useMemo(
    () => ({
      onScrollBeginDrag: () => {
        wait();
        if (moving.current) return;
        moving.current = true;
        monitor.start();
      },
      onScrollEndDrag: () => {
        wait();
        waiting.current = setTimeout(finish, FLING_WAIT_MS);
      },
      onMomentumScrollBegin: wait,
      onMomentumScrollEnd: finish,
    }),
    [finish, monitor, wait],
  );
}
