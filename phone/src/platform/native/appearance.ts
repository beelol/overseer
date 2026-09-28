import { Appearance } from 'react-native';

import { SUPPORTED, defineCapability } from '../capability';
import type { AppearanceCapability, ColorScheme } from '../capabilities/appearance';
import { createLive } from '../live';

function read(): ColorScheme {
  // The system can leave the scheme unspecified; Overseer Light is the theme for that.
  return Appearance.getColorScheme() === 'dark' ? 'dark' : 'light';
}

/** The system's light or dark setting, from React Native's own Appearance module. */
export function createAppearance(): AppearanceCapability {
  const scheme = createLive<ColorScheme>(read());
  // The listener lives as long as the app. It reads the setting again instead of trusting the
  // event's payload, so the value is right even when events arrive out of order.
  Appearance.addChangeListener(() => scheme.set(read()));
  return defineCapability<AppearanceCapability>('appearance', async () => SUPPORTED, {
    get: scheme.get,
    subscribe: scheme.subscribe,
  });
}
