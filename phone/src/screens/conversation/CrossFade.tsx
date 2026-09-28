import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { View, type StyleProp, type ViewStyle } from 'react-native';
import Animated, { cancelAnimation, useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated';

import { useMotion } from '@/motion';
import { makeStyles } from '@/ui';

const useStyles = makeStyles(() => ({
  leaving: { position: 'absolute', top: 0, left: 0, right: 0, bottom: 0 },
}));

export interface CrossFadeProps {
  /** What is shown, by name: when the name changes, the old fades out while the new fades in. */
  readonly value: string;
  readonly children: ReactNode;
  readonly style?: StyleProp<ViewStyle>;
}

/** A status that changes: the word before fades out under the word after (AC-137). */
export function CrossFade({ value, children, style }: CrossFadeProps) {
  const styles = useStyles();
  const motion = useMotion();
  const progress = useSharedValue(1);
  const drawn = useRef({ value, children });
  const [leaving, setLeaving] = useState<ReactNode>(null);
  const lasts = motion.tokens.status.change;

  useLayoutEffect(() => {
    const before = drawn.current;
    drawn.current = { value, children };
    if (before.value === value) return undefined;
    setLeaving(before.children);
    progress.set(0);
    progress.set(withTiming(1, motion.timing(lasts)));
    const timer = setTimeout(() => setLeaving(null), lasts);
    return () => clearTimeout(timer);
    // The children of the same value are drawn as they are; only a new value fades.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);

  useEffect(() => {
    drawn.current = { value, children };
  });
  useEffect(() => () => cancelAnimation(progress), [progress]);

  const entering = useAnimatedStyle(() => ({ opacity: progress.value }));
  const going = useAnimatedStyle(() => ({ opacity: 1 - progress.value }));
  return (
    <View style={style}>
      <Animated.View style={entering}>{children}</Animated.View>
      {leaving !== null ? (
        <Animated.View pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={[styles.leaving, going]}>
          {leaving}
        </Animated.View>
      ) : null}
    </View>
  );
}
