import { Stack } from 'expo-router';
import { StatusBar } from 'expo-status-bar';
import * as SystemUI from 'expo-system-ui';
import { useEffect, useMemo } from 'react';

import { GATEWAY_DISCOVERY } from '@/config';
import { PlatformProvider } from '@/platform';
import { createNativeCapabilities } from '@/platform/native';
import { useTheme } from '@/theme';

// The one place the device's capabilities are created. Everything below receives them
// through the provider, exactly as tests receive the fakes.
const capabilities = createNativeCapabilities({ discovery: GATEWAY_DISCOVERY });

export default function RootLayout() {
  return (
    <PlatformProvider capabilities={capabilities}>
      <ThemedStack />
    </PlatformProvider>
  );
}

function ThemedStack() {
  const theme = useTheme();
  const background = theme.colors.bg;

  // The window behind the screens shows during rotation and transitions; it follows the theme.
  useEffect(() => {
    SystemUI.setBackgroundColorAsync(background).catch(() => undefined);
  }, [background]);

  const screenOptions = useMemo(
    () => ({ headerShown: false, contentStyle: { backgroundColor: background } }),
    [background],
  );

  return (
    <>
      <StatusBar style={theme.scheme === 'dark' ? 'light' : 'dark'} />
      <Stack screenOptions={screenOptions} />
    </>
  );
}
