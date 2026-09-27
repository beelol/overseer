import { useEffect, useMemo, type ReactNode } from 'react';
import type { StyleProp, ViewStyle } from 'react-native';
import Animated, { cancelAnimation, Easing, useAnimatedStyle, useSharedValue, withRepeat, withSpring, withTiming, type WithSpringConfig, type WithTimingConfig } from 'react-native-reanimated';

import { useCapabilities, useLive } from '@/platform';
import { useTheme, type Theme } from '@/theme';

export interface Motion {
  /** True when the system asks for less movement: everything fades and nothing travels. */
  readonly reduced: boolean;
  readonly tokens: Theme['phone']['motion'];
  /** The easing curve of the VS Code themes. */
  readonly ease: WithTimingConfig['easing'];
  /** A timing of `duration` milliseconds (a token) on the app's curve. */
  timing(duration: number): WithTimingConfig;
  spring(name: keyof Theme['phone']['motion']['spring']): WithSpringConfig;
  /** How far a thing travels: `distance` (a token), or nothing with Reduce Motion. */
  travel(distance: number): number;
}

export function useMotion(): Motion {
  const theme = useTheme();
  const reduced = useLive(useCapabilities().reduceMotion);
  return useMemo(() => {
    const [a, b, c, d] = theme.motion.ease;
    const ease = Easing.bezier(a, b, c, d);
    return {
      reduced,
      tokens: theme.phone.motion,
      ease,
      timing: (duration) => ({ duration, easing: ease }),
      spring: (name) => ({ ...theme.phone.motion.spring[name], reduceMotion: undefined }),
      travel: (distance) => (reduced ? 0 : distance),
    };
  }, [theme, reduced]);
}

export interface ArriveProps {
  readonly children: ReactNode;
  readonly style?: StyleProp<ViewStyle>;
  /** False draws the thing in place with no motion: for what was already there. */
  readonly animate?: boolean;
  /** Where it comes from: below (a message), above (a line under the header). */
  readonly from?: 'below' | 'above';
}

/** Something that arrives: it fades in and travels a short way from where it came. */
export function Arrive({ children, style, animate = true, from = 'below' }: ArriveProps) {
  const motion = useMotion();
  const progress = useSharedValue(animate ? 0 : 1);
  const distance = motion.travel(motion.tokens.distance.arrive) * (from === 'below' ? 1 : -1);
  useEffect(() => {
    if (animate) progress.set(withTiming(1, motion.timing(motion.tokens.message.arrive)));
    return () => cancelAnimation(progress);
  }, [animate, motion, progress]);
  const animated = useAnimatedStyle(() => ({ opacity: progress.value, transform: [{ translateY: (1 - progress.value) * distance }] }));
  return <Animated.View style={[style, animated]}>{children}</Animated.View>;
}

export interface PulseProps {
  readonly children: ReactNode;
  readonly style?: StyleProp<ViewStyle>;
  /** False holds still. */
  readonly active?: boolean;
}

/** The needs-you pulse: a slow breath. With Reduce Motion it holds still. */
export function Pulse({ children, style, active = true }: PulseProps) {
  const motion = useMotion();
  const level = useSharedValue(1);
  const moving = active && !motion.reduced;
  useEffect(() => {
    if (moving) level.set(withRepeat(withTiming(motion.tokens.pulse.low, { duration: motion.tokens.pulse.loop / 2, easing: Easing.inOut(Easing.ease) }), -1, true));
    else level.set(withSpring(1, motion.spring('gentle')));
    return () => cancelAnimation(level);
  }, [moving, motion, level]);
  const animated = useAnimatedStyle(() => ({ opacity: level.value }));
  return <Animated.View style={[style, animated]}>{children}</Animated.View>;
}
