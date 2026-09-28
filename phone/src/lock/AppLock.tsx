import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Image, View } from 'react-native';

import { useCapabilities, useLive, type SyncStore } from '@/platform';
import { useTheme } from '@/theme';
import { Button, makeStyles, Txt } from '@/ui';

import type { SafetySettings } from '../screens/settings/safety';

const MARKS = {
  dark: require('../../assets/launch-mark-dark.png'),
  light: require('../../assets/launch-mark-light.png'),
} as const;

export const LOCK_WORDS = {
  title: 'Overseer is locked',
  reason: 'Unlock Overseer',
  unlock: 'Unlock',
  failed: 'The unlock did not work. Try again.',
} as const;

const useStyles = makeStyles((theme) => ({
  cover: { position: 'absolute', top: 0, right: 0, bottom: 0, left: 0, backgroundColor: theme.colors.bg, alignItems: 'center', justifyContent: 'center', gap: theme.space[4], paddingHorizontal: theme.space[6] },
  mark: { width: theme.phone.size.logo.door / 2, height: theme.phone.size.logo.door / 2 },
}));

function wanted(store: SyncStore<SafetySettings>): boolean {
  try {
    return store.get('appLock') === true;
  } catch {
    return false;
  }
}

export interface AppLockProps {
  /** True while the door still covers the app: the system's prompt waits until it has opened. */
  readonly behindDoor: boolean;
}

/**
 * The app lock (AC-130), off unless the owner turned it on in Settings. When on, the app is
 * covered from its first frame and whenever it leaves the screen (so the app switcher's picture
 * shows nothing), and the device's own unlock is asked for once each time the app comes back;
 * Unlock asks again. A device that can no longer unlock (its passcode was removed) is not
 * locked out: the setting could not have been turned on there.
 */
export function AppLock({ behindDoor }: AppLockProps) {
  const styles = useStyles();
  const { scheme } = useTheme();
  const { keyValue, deviceUnlock, appState } = useCapabilities();
  const store = useMemo(() => keyValue.scope<SafetySettings>('settings'), [keyValue]);
  const [locked, setLocked] = useState(() => wanted(store));
  const [asking, setAsking] = useState(false);
  const [failed, setFailed] = useState(false);
  const asked = useRef(false);
  const phase = useLive(appState);

  // Leaving the screen covers the app again and lets the next return ask once more.
  useEffect(
    () =>
      appState.subscribe((now) => {
        if (now !== 'background') return;
        asked.current = false;
        if (wanted(store)) setLocked(true);
      }),
    [appState, store],
  );

  const ask = useCallback(async () => {
    asked.current = true;
    setAsking(true);
    setFailed(false);
    try {
      const support = await deviceUnlock.support();
      if (!support.supported) {
        setLocked(false);
        return;
      }
      const result = await deviceUnlock.unlock({ reason: LOCK_WORDS.reason });
      if (result.ok) setLocked(false);
      else if (result.cause !== 'cancelled') setFailed(true);
    } catch {
      setFailed(true);
    } finally {
      setAsking(false);
    }
  }, [deviceUnlock]);

  useEffect(() => {
    if (locked && !behindDoor && phase === 'foreground' && !asked.current) void ask();
  }, [locked, behindDoor, phase, ask]);

  if (!locked) return null;
  return (
    <View testID="lock.screen" style={styles.cover} accessibilityViewIsModal>
      <Image source={MARKS[scheme]} style={styles.mark} resizeMode="contain" accessibilityIgnoresInvertColors />
      <Txt testID="lock.title" kind="title" accessibilityRole="header">
        {LOCK_WORDS.title}
      </Txt>
      {failed ? (
        <Txt testID="lock.failed" kind="small" tone="red" accessibilityRole="alert">
          {LOCK_WORDS.failed}
        </Txt>
      ) : null}
      <Button testID="lock.unlock" label={LOCK_WORDS.unlock} kind="primary" disabled={asking} onPress={() => void ask()} />
    </View>
  );
}
