import Constants from 'expo-constants';
import { Stack, useRouter } from 'expo-router';
import * as SplashScreen from 'expo-splash-screen';
import { StatusBar } from 'expo-status-bar';
import * as SystemUI from 'expo-system-ui';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { StyleSheet } from 'react-native';
import { GestureHandlerRootView } from 'react-native-gesture-handler';

import { GATEWAY_DISCOVERY } from '@/config';
import { coldStart, Door, doorEnabled, seededSlowness, type TestSettings } from '@/door';
import { AppLock } from '@/lock';
import { Notifications } from '@/notifications';
import { perf, persistPerf, type PerfStored } from '@/perf';
import { PlatformProvider, useCapabilities, useLive } from '@/platform';
import { createNativeCapabilities } from '@/platform/native';
import { createSession, holdable, SessionProvider, useSessionValue } from '@/session';
import { useTheme } from '@/theme';

// The system's launch screen stays until the door's first frame is drawn, so nothing flashes.
SplashScreen.preventAutoHideAsync().catch(() => undefined);

// The one place the device's capabilities and the connection are created. Everything below
// receives them through the providers, exactly as tests receive the fakes.
const capabilities = createNativeCapabilities({ discovery: GATEWAY_DISCOVERY });
const session = createSession({ capabilities, app: Constants.expoConfig?.version ?? '0' });
// What the screens read. It holds still while the door opens: the Mac's first answer comes about
// then, and drawing it (every row, new text) took frames from the opening on Android.
const screens = holdable(session);
const test = capabilities.keyValue.scope<TestSettings>('test');
const door = coldStart() && doorEnabled(test);
// Nothing unless the scenario run seeded it, to prove the run notices a slower start.
perf.record('seeded.slow', seededSlowness(test));

const measured = capabilities.keyValue.scope<PerfStored>('perf');

// Started before the first draw: reading what is stored takes a few milliseconds.
session.start().catch(() => undefined);

// When the Mac first answered, and when its state replaced what was stored: the moments the
// door's opening is measured against.
session.subscribe(() => {
  const now = session.getSnapshot();
  if (now.connection === 'online') perf.mark('session.online');
  if (now.ready && !now.fromCache && now.stateAt !== null) perf.mark('session.state');
});

export default function RootLayout() {
  return (
    <GestureHandlerRootView style={styles.fill}>
      <PlatformProvider capabilities={capabilities}>
        <SessionProvider session={screens}>
          <App />
        </SessionProvider>
      </PlatformProvider>
    </GestureHandlerRootView>
  );
}

function App() {
  const theme = useTheme();
  const router = useRouter();
  const { appState, network, launch } = useCapabilities();
  const background = theme.colors.bg;
  const ready = useSessionValue((s) => s.ready);
  const paired = useSessionValue((s) => s.paired);
  const [closed, setClosed] = useState(door);
  const settled = useFirstScreen();

  // The window behind the screens shows during rotation and transitions; it follows the theme.
  useEffect(() => {
    SystemUI.setBackgroundColorAsync(background).catch(() => undefined);
  }, [background]);

  useEffect(() => {
    if (!door) SplashScreen.hideAsync().catch(() => undefined);
  }, []);

  // In front again, or on another network: try the Mac at once. Leaving: store what is known.
  const phase = useLive(appState);
  const net = useLive(network);
  useEffect(() => {
    if (phase === 'foreground') session.wake();
    else {
      session.background();
      persistPerf(measured);
    }
  }, [phase, net]);

  // The Mac removed this phone, or the owner forgot the Mac: pairing is the only way on.
  // The first screen shows it; whatever was open above it is closed.
  const was = useRef(paired);
  useEffect(() => {
    if (ready && was.current && !paired && router.canDismiss()) router.dismissAll();
    was.current = paired;
  }, [ready, paired, router]);

  const screenOptions = useMemo(
    () => ({
      headerShown: false,
      contentStyle: { backgroundColor: background },
      animation: launch.info().conventions.screenEnter,
      animationDuration: theme.phone.motion.screen.push,
      gestureEnabled: true,
      fullScreenGestureEnabled: true,
    }),
    [background, launch, theme],
  );

  // While the door opens, what the Mac sends is taken in but drawn once the door has gone. Never
  // for longer than two openings, whatever happens to the door.
  const opening = door && closed && ready && settled;
  useEffect(() => {
    if (!opening) return;
    screens.hold();
    const timer = setTimeout(() => screens.release(), theme.phone.motion.door.open * 2);
    return () => {
      clearTimeout(timer);
      screens.release();
    };
  }, [opening, theme]);

  const shown = useCallback(() => {
    perf.mark('door.shown');
    SplashScreen.hideAsync().catch(() => undefined);
  }, []);
  const opened = useCallback(() => {
    perf.mark('door.opened');
    setClosed(false);
    persistPerf(measured);
  }, []);

  // With no door (a test turned it off) the first screen's moments are written a little later.
  useEffect(() => {
    const timer = setTimeout(() => persistPerf(measured), theme.phone.motion.test.whole * 2);
    return () => clearTimeout(timer);
  }, [theme]);

  return (
    <>
      <StatusBar style={theme.scheme === 'dark' ? 'light' : 'dark'} />
      <Stack screenOptions={screenOptions} />
      <Notifications />
      <AppLock behindDoor={closed} />
      {closed ? <Door ready={ready && settled} onShown={shown} onOpened={opened} /> : null}
    </>
  );
}

declare function requestIdleCallback(callback: () => void, options?: { timeout: number }): number;
declare function cancelIdleCallback(handle: number): void;

/**
 * True once the first screen is drawn and the app's logic has come to rest after drawing it.
 * The door opens then and not before: what it reveals is already in place, and nothing the
 * screen still has to do holds the opening back.
 */
function useFirstScreen(): boolean {
  const theme = useTheme();
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    if (settled) return;
    let idle: number | null = null;
    const drawn = (): boolean => Object.keys(perf.report().marks).some((name) => name.startsWith('screen.') && name.endsWith('.interactive'));
    const check = (): void => {
      if (idle !== null || !drawn()) return;
      // At rest, or after a moment at the latest: the door never waits long for a busy screen.
      idle = requestIdleCallback(() => setSettled(true), { timeout: theme.phone.motion.door.fade });
    };
    const off = perf.subscribe(check);
    check();
    return () => {
      off();
      if (idle !== null) cancelIdleCallback(idle);
    };
  }, [settled, theme]);
  return settled;
}

const styles = StyleSheet.create({ fill: { flex: 1 } });
