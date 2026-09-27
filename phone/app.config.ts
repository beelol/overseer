import type { ExpoConfig } from 'expo/config';

// The launch colours come from the same source as every other colour in the app.
import { palettes } from '../extension/design/tokens.js';

const IDENTIFIER = 'com.beelol.overseer.phone';

/**
 * The whole native configuration. `ios/` and `android/` are generated from this file by
 * `expo prebuild` and are not kept in the repository.
 */
const config: ExpoConfig = {
  name: 'Overseer',
  slug: 'overseer-phone',
  version: '0.1.0',
  scheme: 'overseer',
  platforms: ['ios', 'android'],
  orientation: 'default',
  userInterfaceStyle: 'automatic',
  icon: './assets/icon.png',
  ios: {
    bundleIdentifier: IDENTIFIER,
    supportsTablet: false,
    infoPlist: {
      NSLocalNetworkUsageDescription:
        'Overseer connects to the Overseer daemon on your Mac over your local network.',
      NSBonjourServices: ['_overseer._tcp'],
    },
  },
  android: {
    package: IDENTIFIER,
    // The template asks for these; the app has no use for them.
    blockedPermissions: [
      'android.permission.READ_EXTERNAL_STORAGE',
      'android.permission.WRITE_EXTERNAL_STORAGE',
      'android.permission.SYSTEM_ALERT_WINDOW',
    ],
    adaptiveIcon: {
      backgroundColor: palettes.dark.chrome,
      foregroundImage: './assets/android-icon-foreground.png',
      monochromeImage: './assets/android-icon-monochrome.png',
    },
  },
  plugins: [
    'expo-router',
    [
      'expo-splash-screen',
      {
        image: './assets/launch-mark-light.png',
        imageWidth: 160,
        resizeMode: 'contain',
        backgroundColor: palettes.light.bg,
        dark: {
          image: './assets/launch-mark-dark.png',
          backgroundColor: palettes.dark.bg,
        },
      },
    ],
    'expo-secure-store',
    'expo-sqlite',
    [
      'expo-camera',
      {
        cameraPermission: 'Overseer uses the camera to scan the pairing code shown on your Mac.',
        microphonePermission: false,
        recordAudioAndroid: false,
      },
    ],
    [
      'expo-local-authentication',
      {
        faceIDPermission:
          'Overseer uses Face ID when you turn on the app lock or confirm a destructive action.',
      },
    ],
    'expo-notifications',
  ],
  experiments: {
    typedRoutes: true,
  },
};

export default config;
