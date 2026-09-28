import { LinearGradient } from 'expo-linear-gradient';
import { useCallback, useEffect, useState } from 'react';
import { Image, StyleSheet, useWindowDimensions, View } from 'react-native';
import Animated, { cancelAnimation, Easing, runOnJS, useAnimatedStyle, useFrameCallback, useSharedValue, withRepeat, withTiming } from 'react-native-reanimated';

import { useMotion } from '@/motion';
import { perf, useFrameMonitor } from '@/perf';
import { useCapabilities } from '@/platform';
import { faded, useTheme, type Theme } from '@/theme';

const MARKS = {
  dark: require('../../assets/launch-mark-dark.png'),
  light: require('../../assets/launch-mark-light.png'),
} as const;

export interface DoorProps {
  /** True once the first screen is drawn from what the phone has stored. The door opens then. */
  readonly ready: boolean;
  /** Called when the door's first frame is on screen: the system's launch screen may go. */
  readonly onShown: () => void;
  /** Called when the door has opened and is gone. */
  readonly onOpened: () => void;
}

/**
 * The door's colours, from the active theme: deep at the far corners, lit towards the seam, the
 * same on both sides of it. The streak along the seam is the theme's accent laid thinly over
 * the background, so it is the same purple in the light theme as in the dark one.
 */
function gradient(theme: Theme): readonly [string, string, string, string, string] {
  const c = theme.colors;
  const streak = faded(c.accent, theme.phone.opacity.doorStreak);
  return [c.chrome, c.bg, streak, c.bg, c.chrome];
}

const STOPS = [0, 0.3, 0.5, 0.7, 1] as const;

/**
 * Where the gradient starts and ends on a face, so that it runs square to the seam: the seam
 * climbs to the right by `degrees`, and the light lies along it.
 */
function across(width: number, height: number, degrees: number): { start: { x: number; y: number }; end: { x: number; y: number } } {
  const angle = (degrees * Math.PI) / 180;
  const nx = Math.sin(angle);
  const ny = Math.cos(angle);
  const reach = (width * nx + height * ny) / 2;
  return {
    start: { x: 0.5 - (nx * reach) / width, y: 0.5 - (ny * reach) / height },
    end: { x: 0.5 + (nx * reach) / width, y: 0.5 + (ny * reach) / height },
  };
}

/**
 * The door (AC-136). On a cold start it is closed over the app from the first frame, in the
 * same place and mode as the system's launch screen: the theme's background and Overseer's mark
 * in grey. Its gradient and the light along its seam fade in while it waits. When the first
 * screen is drawn, the door splits along a diagonal seam and its halves slide apart, the mark
 * splitting with them, over the app already in place. It never takes a touch.
 *
 * Everything that moves runs on the UI thread, so the door holds its frames while the app's
 * logic is busy. With Reduce Motion on, the door fades out instead.
 */
export function Door({ ready, onShown, onOpened }: DoorProps) {
  const theme = useTheme();
  const motion = useMotion();
  const { haptics } = useCapabilities();
  const { width, height } = useWindowDimensions();
  const tokens = motion.tokens.door;

  const open = useSharedValue(0); // 0 closed, 1 open
  const lit = useSharedValue(0); // the gradient and the seam, fading in over the launch screen
  const light = useSharedValue(0); // the light's place along the seam, 0 to 1
  const [shown, setShown] = useState(false);
  const counting = useSharedValue(false); // the frame monitor counts the opening's frames only
  const frames = useFrameMonitor(counting);

  // A square that covers the screen at any angle; the seam is its middle line.
  const side = Math.ceil(Math.hypot(width, height)) + theme.space[10];
  const half = side / 2;

  useEffect(() => {
    lit.set(withTiming(1, motion.timing(tokens.fade)));
    if (!motion.reduced) light.set(withRepeat(withTiming(1, { duration: tokens.lightLoop, easing: Easing.inOut(Easing.ease) }), -1, false));
    return () => {
      cancelAnimation(lit);
      cancelAnimation(light);
    };
  }, [lit, light, motion, tokens]);

  // The opening waits for the UI thread to be calm: the first screen's views are mounted and
  // drawn there a moment after the app's logic has laid them out, and the system's launch screen
  // leaves then too. It starts on the UI thread itself after a few frames on time in a row, or
  // after the fade's time at the latest, so it never waits long.
  const armed = useSharedValue(false);
  const calm = useSharedValue(0);
  const waited = useSharedValue(0);
  const period = useSharedValue(0);
  const startedAt = useSharedValue(0); // the frame the opening started on, UI thread's clock
  const timing = motion.reduced ? motion.timing(tokens.fade) : { duration: tokens.open, easing: motion.ease };
  const longest = tokens.fade;

  const finish = useCallback(() => {
    // The opening, as the UI thread drew it: from the frame it started on to its last frame, and
    // every frame it missed. Timed there, not by the app's logic, which may hear of either late.
    const drawn = frames.stop();
    const { last } = frames.span();
    perf.record('door.opening', last - startedAt.get());
    perf.record('door.frames', drawn.frames);
    perf.record('door.dropped', drawn.dropped);
    perf.record('door.longestFrame', drawn.longest);
    // Where in the opening each late frame came, and how late: what held the UI thread then.
    frames.stalls().forEach((stall, i) => {
      perf.record(`door.stall.${i + 1}.at`, stall.at);
      perf.record(`door.stall.${i + 1}.ms`, stall.ms);
    });
    onOpened();
  }, [frames, onOpened, startedAt]);

  const began = useCallback(
    (at: number, wait: number) => {
      // When it started on the display. Frame times share the clock of `performance.now()`;
      // should a platform's not, the moment the app's logic heard of it stands in.
      const now = performance.now();
      perf.markAt('door.opening', Math.abs(now - at) < 1_000 ? at : now);
      perf.record('door.waited', wait);
      haptics.play('impact');
    },
    [haptics],
  );

  const done = useCallback(
    (finished?: boolean) => {
      'worklet';
      // The door has stopped moving: what the UI thread does from here is not the opening's.
      counting.set(false);
      if (finished) runOnJS(finish)();
    },
    [counting, finish],
  );

  const onFrame = useFrameCallback((info) => {
    'worklet';
    if (!armed.value) return;
    const between = info.timeSincePreviousFrame;
    if (between === null) return;
    waited.value += between;
    // The display's own frame time: the shortest seen, no faster than 120 Hz.
    if (period.value === 0 || (between < period.value && between > 8)) period.value = between;
    calm.value = between <= period.value * 1.5 ? calm.value + 1 : 0;
    if (calm.value < 3 && waited.value < longest) return;
    armed.value = false;
    startedAt.set(info.timestamp);
    counting.set(true);
    open.value = withTiming(1, timing, done);
    runOnJS(began)(info.timestamp, Math.round(waited.value));
  }, false);

  useEffect(() => {
    if (!ready || !shown) return;
    calm.set(0);
    waited.set(0);
    frames.start({ counting: false });
    armed.set(true);
    // Once it has opened it does nothing more, and it goes with the door.
    onFrame.setActive(true);
  }, [ready, shown, frames, armed, calm, waited, onFrame]);

  const reduced = motion.reduced;
  const whole = useAnimatedStyle(() => ({ opacity: reduced ? 1 - open.value : 1 }));
  const upper = useAnimatedStyle(() => ({ transform: [{ translateY: reduced ? 0 : -open.value * half }] }));
  const lower = useAnimatedStyle(() => ({ transform: [{ translateY: reduced ? 0 : open.value * half }] }));
  const glow = useAnimatedStyle(() => ({ opacity: lit.value }));
  const seam = useAnimatedStyle(() => ({ opacity: lit.value * (1 - Math.min(1, open.value * 3)) }));
  const lightWidth = theme.phone.size.doorLight;
  const moving = useAnimatedStyle(() => ({ transform: [{ translateX: -lightWidth + light.value * (side + lightWidth) }] }));

  const mark = theme.phone.size.logo.door;
  const run = across(width, height, tokens.angle);
  const face = (
    <View style={{ width, height, backgroundColor: theme.colors.bg }}>
      <Animated.View style={[StyleSheet.absoluteFill, glow]}>
        <LinearGradient colors={gradient(theme)} locations={STOPS} start={run.start} end={run.end} style={StyleSheet.absoluteFill} />
      </Animated.View>
      <View style={[StyleSheet.absoluteFill, styles.center]}>
        <Image source={MARKS[theme.scheme]} style={{ width: mark, height: mark }} resizeMode="contain" fadeDuration={0} accessibilityIgnoresInvertColors />
      </View>
    </View>
  );
  // What a half carries along its edge of the seam: the light that leaks through, and the
  // lines of its plating. They lie along the seam and travel with the half.
  const line = theme.phone.size.hairline;
  const plating = (edge: 'top' | 'bottom') => (
    <Animated.View pointerEvents="none" style={[StyleSheet.absoluteFill, glow]}>
      <LinearGradient
        colors={edge === 'bottom' ? ['transparent', theme.colors.accent] : [theme.colors.accent, 'transparent']}
        style={{ position: 'absolute', left: 0, right: 0, [edge]: 0, height: theme.space[10], opacity: theme.phone.opacity.scrim }}
      />
      {[theme.space[6], theme.space[8], theme.space[10] * 3].map((distance) => (
        <View key={distance} style={{ position: 'absolute', left: 0, right: 0, [edge]: distance, height: line, backgroundColor: theme.colors.borderStrong }} />
      ))}
    </Animated.View>
  );
  const angle = `${-tokens.angle}deg`;
  const back = `${tokens.angle}deg`;
  const left = (side - width) / 2;

  return (
    <Animated.View
      testID="door"
      pointerEvents="none"
      accessible={false}
      importantForAccessibility="no-hide-descendants"
      style={[StyleSheet.absoluteFill, styles.clip, whole]}
      onLayout={() => {
        if (shown) return;
        setShown(true);
        onShown();
      }}
    >
      <View style={{ position: 'absolute', width: side, height: side, left: (width - side) / 2, top: (height - side) / 2, transform: [{ rotate: angle }] }}>
        <Animated.View style={[{ position: 'absolute', top: 0, left: 0, width: side, height: half }, styles.clip, upper]}>
          <View style={{ position: 'absolute', left, top: half - height / 2, transform: [{ rotate: back }] }}>{face}</View>
          {plating('bottom')}
        </Animated.View>
        <Animated.View style={[{ position: 'absolute', top: half, left: 0, width: side, height: half }, styles.clip, lower]}>
          <View style={{ position: 'absolute', left, top: -height / 2, transform: [{ rotate: back }] }}>{face}</View>
          {plating('top')}
        </Animated.View>
        <Animated.View style={[{ position: 'absolute', top: half - theme.phone.size.doorSeam / 2, left: 0, width: side, height: theme.phone.size.doorSeam, backgroundColor: theme.colors.borderStrong }, styles.clip, seam]}>
          <Animated.View style={[{ width: lightWidth, height: theme.phone.size.doorSeam }, moving]}>
            <LinearGradient colors={['transparent', theme.colors.accent, 'transparent']} start={{ x: 0, y: 0 }} end={{ x: 1, y: 0 }} style={StyleSheet.absoluteFill} />
          </Animated.View>
        </Animated.View>
      </View>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  clip: { overflow: 'hidden' },
  center: { alignItems: 'center', justifyContent: 'center' },
});
