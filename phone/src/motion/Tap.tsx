import { useCallback, type ReactNode } from 'react';
import { Pressable, type GestureResponderEvent, type PressableProps, type StyleProp, type ViewStyle } from 'react-native';
import Animated, { useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated';

import { useCapabilities, type HapticMoment } from '@/platform';
import { useTheme } from '@/theme';

import { useMotion } from './motion';

const AnimatedPressable = Animated.createAnimatedComponent(Pressable);

export interface TapProps extends Omit<PressableProps, 'style' | 'children' | 'testID' | 'accessibilityLabel'> {
  /** `screen.part`, as the app's brief names it. Every control has one. */
  readonly testID: string;
  /** What VoiceOver and TalkBack say. Every control has one. */
  readonly accessibilityLabel: string;
  readonly children: ReactNode;
  readonly style?: StyleProp<ViewStyle>;
  /** The haptic of this moment, in the platform's own style. None unless given. */
  readonly haptic?: HapticMoment;
  /** False leaves the size alone when pressed (rows of a list): it dims only. */
  readonly scales?: boolean;
}

/**
 * Anything that is pressed. It answers the finger in the same frame, on the UI thread, whatever
 * the app's logic is doing: it dims and gives a little, and comes back when let go.
 */
export function Tap({ testID, accessibilityLabel, children, style, haptic, scales = true, disabled, onPress, onPressIn, onPressOut, ...rest }: TapProps) {
  const theme = useTheme();
  const motion = useMotion();
  const { haptics } = useCapabilities();
  const pressed = useSharedValue(0);
  const { press } = motion.tokens;
  const low = theme.phone.opacity.pressed;
  const small = scales && !motion.reduced ? press.scale : 1;

  const animated = useAnimatedStyle(() => ({
    opacity: 1 - pressed.value * (1 - low),
    transform: [{ scale: 1 - pressed.value * (1 - small) }],
  }));

  const pressIn = useCallback(
    (event: GestureResponderEvent) => {
      pressed.set(withTiming(1, { duration: press.in }));
      onPressIn?.(event);
    },
    [pressed, press.in, onPressIn],
  );
  const pressOut = useCallback(
    (event: GestureResponderEvent) => {
      pressed.set(withTiming(0, { duration: press.out }));
      onPressOut?.(event);
    },
    [pressed, press.out, onPressOut],
  );
  const pressedNow = useCallback(
    (event: GestureResponderEvent) => {
      if (haptic) haptics.play(haptic);
      onPress?.(event);
    },
    [haptic, haptics, onPress],
  );

  return (
    <AnimatedPressable
      accessibilityRole="button"
      {...rest}
      testID={testID}
      accessibilityLabel={accessibilityLabel}
      accessibilityState={{ disabled: Boolean(disabled), ...rest.accessibilityState }}
      disabled={disabled}
      hitSlop={theme.space[2]}
      onPress={pressedNow}
      onPressIn={pressIn}
      onPressOut={pressOut}
      style={[style, animated, disabled ? { opacity: theme.phone.opacity.disabled } : null]}
    >
      {children}
    </AnimatedPressable>
  );
}
