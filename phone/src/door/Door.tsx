import { LinearGradient } from 'expo-linear-gradient';
import { useEffect, useState } from 'react';
import { Image, StyleSheet, useWindowDimensions, View } from 'react-native';
import Animated, { cancelAnimation, Easing, runOnJS, useAnimatedStyle, useSharedValue, withDelay, withRepeat, withTiming } from 'react-native-reanimated';

import { useMotion } from '@/motion';
import { useCapabilities } from '@/platform';
import { useTheme, type Theme } from '@/theme';

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

/** The door's colours, from the active theme: deep at the edges, lit along the seam. */
function gradient(theme: Theme): readonly [string, string, string, string] {
  const c = theme.colors;
  return [c.chrome, c.bg, c.accentSoft, c.raised2];
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

  useEffect(() => {
    if (!ready || !shown) return;
    haptics.play('impact');
    const done = (finished?: boolean) => {
      'worklet';
      if (finished) runOnJS(onOpened)();
    };
    open.set(motion.reduced ? withTiming(1, motion.timing(tokens.fade), done) : withDelay(0, withTiming(1, { duration: tokens.open, easing: motion.ease }, done)));
  }, [ready, shown, open, motion, tokens, haptics, onOpened]);

  const reduced = motion.reduced;
  const whole = useAnimatedStyle(() => ({ opacity: reduced ? 1 - open.value : 1 }));
  const upper = useAnimatedStyle(() => ({ transform: [{ translateY: reduced ? 0 : -open.value * half }] }));
  const lower = useAnimatedStyle(() => ({ transform: [{ translateY: reduced ? 0 : open.value * half }] }));
  const glow = useAnimatedStyle(() => ({ opacity: lit.value }));
  const seam = useAnimatedStyle(() => ({ opacity: lit.value * (1 - Math.min(1, open.value * 3)) }));
  const lightWidth = theme.phone.size.doorLight;
  const moving = useAnimatedStyle(() => ({ transform: [{ translateX: -lightWidth + light.value * (side + lightWidth) }] }));

  const mark = theme.phone.size.logo.door;
  const face = (
    <View style={{ width, height, backgroundColor: theme.colors.bg }}>
      <Animated.View style={[StyleSheet.absoluteFill, glow]}>
        <LinearGradient colors={gradient(theme)} locations={[0, 0.38, 0.5, 1]} start={{ x: 0, y: 0 }} end={{ x: 1, y: 1 }} style={StyleSheet.absoluteFill} />
      </Animated.View>
      <View style={[StyleSheet.absoluteFill, styles.center]}>
        <Image source={MARKS[theme.scheme]} style={{ width: mark, height: mark }} resizeMode="contain" fadeDuration={0} accessibilityIgnoresInvertColors />
      </View>
    </View>
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
        </Animated.View>
        <Animated.View style={[{ position: 'absolute', top: half, left: 0, width: side, height: half }, styles.clip, lower]}>
          <View style={{ position: 'absolute', left, top: -height / 2, transform: [{ rotate: back }] }}>{face}</View>
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
