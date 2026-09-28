import { useCallback, useEffect, useRef, type ReactNode } from 'react';
import { Modal, Pressable, View } from 'react-native';
import Animated, { useAnimatedStyle, useSharedValue, withSpring, withTiming } from 'react-native-reanimated';
import { SafeAreaView } from 'react-native-safe-area-context';

import { useMotion } from '@/motion';
import { useCapabilities } from '@/platform';
import { useTheme } from '@/theme';

import { Row } from './Rows';
import type { IconName } from './Icon';
import { makeStyles } from './styles';
import { Txt } from './Txt';

const useStyles = makeStyles((theme) => ({
  fill: { flex: 1, justifyContent: 'flex-end' },
  scrim: { position: 'absolute', top: 0, bottom: 0, left: 0, right: 0, backgroundColor: theme.colors.shadow },
  sheet: { backgroundColor: theme.colors.raised, borderTopLeftRadius: theme.radius.card, borderTopRightRadius: theme.radius.card, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.border, paddingTop: theme.space[2] },
  grip: { alignSelf: 'center', width: theme.space[10], height: theme.space[1], borderRadius: theme.radius.pill, backgroundColor: theme.colors.borderStrong, marginBottom: theme.space[2] },
  title: { paddingHorizontal: theme.space[4], paddingVertical: theme.space[2] },
  message: { paddingHorizontal: theme.space[4], paddingBottom: theme.space[3] },
}));

export interface SheetProps {
  readonly testID: string;
  readonly open: boolean;
  readonly onClose: () => void;
  readonly title?: string;
  readonly message?: string;
  readonly children: ReactNode;
  /** Called once the sheet is gone from the screen (iOS says so when its modal is dismissed). */
  readonly onDismissed?: () => void;
}

/** A sheet from the bottom, within reach of a thumb: a menu, a choice, a question asked once. */
export function Sheet({ testID, open, onClose, title, message, children, onDismissed }: SheetProps) {
  const styles = useStyles();
  const theme = useTheme();
  const motion = useMotion();
  const { haptics } = useCapabilities();
  const shown = useSharedValue(0);
  useEffect(() => {
    if (open) {
      haptics.play('impact');
      shown.set(motion.reduced ? withTiming(1, motion.timing(motion.tokens.sheet.open)) : withSpring(1, motion.spring('snappy')));
    } else shown.set(0);
  }, [open, shown, motion, haptics]);
  const distance = motion.travel(theme.space[10] * theme.space[2]);
  const sheet = useAnimatedStyle(() => ({ opacity: Math.min(1, shown.value * 2), transform: [{ translateY: (1 - shown.value) * distance }] }));
  const scrim = useAnimatedStyle(() => ({ opacity: shown.value }));
  return (
    <Modal visible={open} transparent animationType="none" onRequestClose={onClose} {...(onDismissed ? { onDismiss: onDismissed } : {})} statusBarTranslucent supportedOrientations={['portrait', 'landscape']}>
      <View style={styles.fill} accessibilityViewIsModal>
        <Animated.View style={[styles.scrim, scrim]}>
          <Pressable testID={`${testID}.close`} accessibilityLabel="Dismiss" accessibilityRole="button" style={styles.fill} onPress={onClose} />
        </Animated.View>
        <Animated.View testID={testID} style={[styles.sheet, sheet]}>
          <SafeAreaView edges={['bottom', 'left', 'right']}>
            <View style={styles.grip} />
            {title ? (
              <Txt kind="strong" accessibilityRole="header" style={styles.title}>
                {title}
              </Txt>
            ) : null}
            {message ? (
              <Txt kind="label" tone="muted" style={styles.message}>
                {message}
              </Txt>
            ) : null}
            {children}
          </SafeAreaView>
        </Animated.View>
      </View>
    </Modal>
  );
}

export interface MenuItem {
  readonly id: string;
  readonly label: string;
  readonly detail?: string;
  readonly icon?: IconName;
  /** True for what cannot be undone. */
  readonly danger?: boolean;
  /**
   * True for an item that shows a screen of the system (the photo picker, the camera): it runs
   * once the sheet is gone, since closing the sheet's modal would take that screen down with it.
   */
  readonly afterClose?: boolean;
  readonly onPress: () => void;
}

export interface MenuProps extends Omit<SheetProps, 'children'> {
  readonly items: readonly MenuItem[];
}

/** A sheet of actions. Choosing one closes it. Test ids are `<sheet>.<item id>`. */
export function Menu({ items, ...sheet }: MenuProps) {
  const theme = useTheme();
  const pending = useRef<(() => void) | null>(null);
  const runPending = useCallback(() => {
    const run = pending.current;
    pending.current = null;
    run?.();
  }, []);
  // Where the platform does not say the modal is gone, a sheet's time to open is ample for it to close.
  const { open } = sheet;
  useEffect(() => {
    if (open || pending.current === null) return;
    const timer = setTimeout(runPending, theme.phone.motion.sheet.open);
    return () => clearTimeout(timer);
  }, [open, runPending, theme]);
  return (
    <Sheet {...sheet} onDismissed={runPending}>
      {items.map((item, index) => (
        <Row
          key={item.id}
          testID={`${sheet.testID}.${item.id}`}
          label={item.label}
          {...(item.detail ? { detail: item.detail } : {})}
          {...(item.icon ? { icon: item.icon } : {})}
          tone={item.danger ? 'red' : 'text'}
          divided={index > 0}
          right={<View />}
          onPress={() => {
            if (item.afterClose) {
              pending.current = item.onPress;
              sheet.onClose();
              return;
            }
            sheet.onClose();
            item.onPress();
          }}
        />
      ))}
    </Sheet>
  );
}

export interface ConfirmProps extends Omit<SheetProps, 'children' | 'title' | 'message'> {
  /** The question, naming what will be lost. */
  readonly question: string;
  readonly detail?: string;
  /** The words of the action ("Put back", "Forget this Mac"). */
  readonly confirm: string;
  readonly danger?: boolean;
  readonly onConfirm: () => void;
}

/** A question asked once before something that cannot be undone. */
export function Confirm({ question, detail, confirm, danger = true, onConfirm, ...sheet }: ConfirmProps) {
  return (
    <Menu
      {...sheet}
      title={question}
      {...(detail ? { message: detail } : {})}
      items={[
        { id: 'confirm', label: confirm, danger, onPress: onConfirm },
        { id: 'cancel', label: 'Cancel', onPress: () => undefined },
      ]}
    />
  );
}
