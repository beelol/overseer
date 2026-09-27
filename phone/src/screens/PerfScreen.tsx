import { useCallback, useEffect, useState, useSyncExternalStore } from 'react';
import { ScrollView } from 'react-native';
import Animated, { Easing, useAnimatedStyle, useSharedValue, withRepeat, withTiming } from 'react-native-reanimated';

import { perf, persistBusy, persistPerf, startup, useFrameMonitor, type FrameStats, type PerfStored } from '@/perf';
import { useCapabilities } from '@/platform';
import { Actions, Button, makeStyles, Screen, Txt } from '@/ui';
import { useTheme } from '@/theme';

const useStyles = makeStyles((theme) => ({
  content: { padding: theme.space[4], gap: theme.space[4] },
  track: { height: theme.space[2], borderRadius: theme.radius.pill, backgroundColor: theme.colors.raised2, overflow: 'hidden' },
  runner: { width: theme.space[10], height: theme.space[2], borderRadius: theme.radius.pill, backgroundColor: theme.colors.accent },
}));

/** Keeps the app's logic busy, the way a heavy batch of events would. */
function busy(ms: number): void {
  const end = Date.now() + ms;
  let n = 0;
  while (Date.now() < end) n += Math.sqrt(n + 1);
}

const snapshot = (): string => JSON.stringify({ startup: startup(), ...perf.report(), measures: undefined });

/**
 * The app's measurements, as text the scenario run reads, and the busy-logic test: an animation
 * runs on the UI thread while the app's logic is held for half a second, and the frames are
 * counted. Reached by `overseer://perf`; nothing in the app leads here.
 */
export function PerfScreen() {
  const styles = useStyles();
  const theme = useTheme();
  const monitor = useFrameMonitor();
  const { keyValue } = useCapabilities();
  const [frames, setFrames] = useState<FrameStats | null>(null);
  const [version, setVersion] = useState(0);
  const report = useSyncExternalStore(
    (listener) => perf.subscribe(listener),
    () => version,
  );
  const travel = useSharedValue(0);
  const moving = useAnimatedStyle(() => ({ transform: [{ translateX: travel.value * theme.space[10] * theme.space[1] }] }));

  useEffect(() => perf.subscribe(() => setVersion((v) => v + 1)), []);

  const run = useCallback(() => {
    setFrames(null);
    travel.set(0);
    travel.set(withRepeat(withTiming(1, { duration: theme.phone.motion.door.open, easing: Easing.inOut(Easing.ease) }), -1, true));
    monitor.start();
    // The animation alone, then half a second with the logic held, then the animation alone.
    const { lead, busy: held, whole } = theme.phone.motion.test;
    setTimeout(() => busy(held), lead);
    setTimeout(() => {
      const drawn = monitor.stop();
      setFrames(drawn);
      const stored = keyValue.scope<PerfStored>('perf');
      persistBusy(stored, drawn);
      persistPerf(stored);
    }, whole);
  }, [monitor, travel, theme, keyValue]);

  return (
    <Screen id="perf" title="Measurements" connection={false}>
      <ScrollView contentContainerStyle={styles.content}>
        <Actions>
          <Button testID="perf.busy" label="Hold the logic for 500 ms" onPress={run} />
          <Button testID="perf.reset" label="Reset" kind="quiet" onPress={() => perf.reset()} />
        </Actions>
        <Animated.View style={styles.track}>
          <Animated.View style={[styles.runner, moving]} />
        </Animated.View>
        <Txt testID="perf.frames" kind="mono" selectable>
          {frames ? JSON.stringify(frames) : 'frames: not run'}
        </Txt>
        <Txt testID="perf.json" kind="mono" selectable accessibilityLabel={`${report} ${snapshot()}`}>
          {snapshot()}
        </Txt>
      </ScrollView>
    </Screen>
  );
}
