import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { View } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, { runOnJS, useAnimatedStyle, useSharedValue, withSpring, withTiming } from 'react-native-reanimated';

import { Tap, useMotion } from '@/motion';
import { useTheme } from '@/theme';
import { Icon, makeStyles, Txt, type IconName } from '@/ui';

export interface SwipeActionProps {
  /** The row this belongs to. Rows of a list are used again: another id closes it. */
  readonly id: string;
  /** False leaves the row as it is: nothing to swipe (a watch-only phone). */
  readonly enabled: boolean;
  /** The test id of the action; the gesture's is `<testID>.swipe`. */
  readonly testID: string;
  readonly label: string;
  readonly accessibilityLabel: string;
  readonly icon: IconName;
  readonly onAction: () => void;
  readonly children: ReactNode;
}

const useStyles = makeStyles((theme) => ({
  frame: { overflow: 'hidden' },
  action: { position: 'absolute', top: 0, bottom: 0, right: 0, backgroundColor: theme.colors.accentStrong },
  press: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: theme.space[1], paddingHorizontal: theme.space[2] },
}));

/**
 * A row that is swiped to the left for one action. The row follows the finger and the action
 * comes in at its edge; let go past half of it and it stays open, swipe far and it is done.
 * With Reduce Motion nothing travels: the same swipe fades the action in over the row's end.
 */
export function SwipeAction({ id, enabled, testID, label, accessibilityLabel, icon, onAction, children }: SwipeActionProps) {
  const styles = useStyles();
  const theme = useTheme();
  const motion = useMotion();
  const width = theme.phone.size.touch * 2;
  const far = width * 2;
  const slop = theme.space[3];
  const reduced = motion.reduced;
  const spring = useMemo(() => motion.spring('snappy'), [motion]);
  const fade = useMemo(() => motion.timing(motion.tokens.status.change), [motion]);

  /** How far the row has travelled: 0, or less while it is open or under the finger. */
  const travel = useSharedValue(0);
  /** 1 while the action is shown and can be pressed. */
  const shown = useSharedValue(0);
  const from = useSharedValue(0);
  const [open, setOpen] = useState(false);

  const close = useCallback(() => {
    travel.set(reduced ? 0 : withSpring(0, spring));
    shown.set(withTiming(0, fade));
    setOpen(false);
  }, [travel, shown, reduced, spring, fade]);

  // Another agent took this row: it starts closed, with no motion.
  const [owner, setOwner] = useState(id);
  if (owner !== id) {
    setOwner(id);
    setOpen(false);
  }
  useEffect(() => {
    travel.set(0);
    shown.set(0);
  }, [id, travel, shown]);

  const act = useCallback(() => {
    close();
    onAction();
  }, [close, onAction]);

  const pan = useMemo(
    () =>
      Gesture.Pan()
        .enabled(enabled)
        .withTestId(`${testID}.swipe`)
        .activeOffsetX([-slop, slop])
        .failOffsetY([-slop, slop])
        .onBegin(() => {
          'worklet';
          from.set(shown.get() > 0 ? -width : 0);
        })
        .onUpdate((event) => {
          'worklet';
          if (!reduced) travel.set(Math.min(0, Math.max(-far * 2, from.get() + event.translationX)));
        })
        .onEnd((event) => {
          'worklet';
          const at = from.get() + event.translationX;
          const stays = at > -far && at <= -width / 2;
          travel.set(reduced ? 0 : withSpring(stays ? -width : 0, spring));
          shown.set(withTiming(stays ? 1 : 0, fade));
          runOnJS(setOpen)(stays);
          if (at <= -far) runOnJS(onAction)();
        }),
    [enabled, testID, slop, width, far, reduced, spring, fade, from, travel, shown, onAction],
  );

  const row = useAnimatedStyle(() => ({ transform: [{ translateX: travel.value }] }));
  const action = useAnimatedStyle(() =>
    reduced ? { width, opacity: shown.value, transform: [{ translateX: 0 }] } : { width: Math.max(width, -travel.value), opacity: 1, transform: [{ translateX: Math.max(0, width + travel.value) }] },
  );

  if (!enabled) return <View>{children}</View>;
  return (
    <View style={styles.frame}>
      <GestureDetector gesture={pan}>
        <Animated.View testID={`${testID}.travel`} style={row}>
          {children}
        </Animated.View>
      </GestureDetector>
      <Animated.View testID={`${testID}.shown`} style={[styles.action, action]} pointerEvents={open ? 'auto' : 'none'} accessibilityElementsHidden={!open} importantForAccessibility={open ? 'auto' : 'no-hide-descendants'}>
        <Tap testID={testID} accessibilityLabel={accessibilityLabel} haptic="confirm" scales={false} onPress={act} style={styles.press}>
          <Icon name={icon} size="lg" tone="onAccent" />
          <Txt kind="small" tone="onAccent" numberOfLines={1}>
            {label}
          </Txt>
        </Tap>
      </Animated.View>
    </View>
  );
}
