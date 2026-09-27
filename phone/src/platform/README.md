# The platform layer

Everything that differs between iOS and Android sits here, behind one typed, generic interface
per capability ([AC-134](../../../docs/overseer-rfc.md)). Screens and shared code use the
interfaces only and never ask which platform they run on.

## The shape

```
capability.ts        Capability<Name, Api>, Support, CapabilityUnsupportedError
live.ts              Live<T>: a value that changes while the app runs
stores.ts            AsyncStore<Schema>, SyncStore<Schema>: stores typed by what they hold
capabilities/        one interface per capability, with its doc comments
native/              the device's implementations (created once, in app/_layout.tsx)
  *.ios.ts           chosen by the bundler on iOS
  *.android.ts       chosen by the bundler on Android
fake/                in-memory, deterministic implementations for tests
context.tsx          <PlatformProvider>, useCapabilities(), useLive()
```

- **One generic base.** `Capability<Name, Api>` is the API plus a `name` and a `support()` check.
  `Support` is `{ supported: true }` or `{ supported: false, reason }`.
- **Honest gaps.** A capability a platform lacks reports unsupported with the reason. Used
  anyway, it throws or rejects `CapabilityUnsupportedError` with the same reason. Two
  exceptions are written in their interfaces: `haptics.play` does nothing (a missing buzz is
  not an error), and `discovery`'s manual and platform addresses work while browsing is
  unsupported.
- **Generic where the capabilities are alike.** The appearance, Reduce Motion, the app's state
  and the network are all `Live<T>`, followed by one hook, `useLive`. The keystore and the
  key-value store are typed views, `scope<Schema>(namespace)`, over a string backend.
- **Injected, not imported.** The app's root creates the device's capabilities and gives them
  to `<PlatformProvider>`. Tests give it `createFakePlatform().capabilities` instead. Nothing
  else changes between a device and a test.

## Capabilities

| Capability     | iOS                                                                                                           | Android                                                                                                                           | Fake                                                                                       | Library or own code                                                                                         | Why                                                                                                                                                                   |
| -------------- | ------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `secretStore`  | Keychain, readable after the first unlock, this device only                                                   | Keystore-encrypted preferences, excluded from backup                                                                              | A map in memory, same scopes and key rules                                                 | `expo-secure-store`                                                                                         | One maintained library covers both keystores; keys never leave the device on either.                                                                                  |
| `keyValue`     | SQLite in the app's files                                                                                     | SQLite in the app's files                                                                                                         | A map in memory, same scopes and JSON encoding                                             | `expo-sqlite` (`kv-store`)                                                                                  | Reads are synchronous, so the first screen can be drawn from the cache at once. Maintained by Expo, and SQLite is what the event cache will use.                      |
| `random`       | `SecRandomCopyBytes`                                                                                          | `SecureRandom`                                                                                                                    | A seeded generator (mulberry32): the same seed gives the same bytes                        | `expo-crypto`                                                                                               | Synchronous secure bytes and UUIDs on both platforms.                                                                                                                 |
| `discovery`    | Manual addresses and `127.0.0.1` on the simulator work. Browsing with Bonjour: **unsupported**, not built yet | Manual addresses and `10.0.2.2` on the emulator work. Browsing: **unsupported**; the emulator cannot see the local network at all | The real address logic over the fake stores, with a browser the test drives                | Own code (`addresses.ts`, `discoveryCore.ts`); the browser is per platform                                  | Parsing and keeping addresses is the same everywhere. A browsing library is chosen with the real iPhone (AC-120), where the local network permission can be verified. |
| `push`         | Apple's push service: permission, device token, categories with actions, handlers                             | **Unsupported** in this gate: Android push needs a Firebase project and comes with the relay                                      | Permission asked once, a fixed token, recorded categories, notifications the test delivers | `expo-notifications` (iOS); own stub (Android), and the library is left out of the Android build            | One maintained library covers tokens, categories and responses. The Android file reports the gap instead of pretending.                                               |
| `deviceUnlock` | Face ID, Touch ID, passcode                                                                                   | Fingerprint, face, PIN                                                                                                            | Answers what the test tells it; records each request                                       | `expo-local-authentication`                                                                                 | One maintained library covers both.                                                                                                                                   |
| `camera`       | The camera, QR codes only; **unsupported** on the simulator                                                   | The camera, QR codes only; **unsupported** on the emulator                                                                        | A scanner that draws nothing and reads what the test holds up                              | `expo-camera`                                                                                               | One maintained library covers both; typing the code is the path on simulators.                                                                                        |
| `haptics`      | The system's feedback generators; **unsupported** on the simulator                                            | The system's view haptics; **unsupported** on the emulator                                                                        | Records the moments played                                                                 | `expo-haptics`, with the moments mapped per platform (`hapticsMoments.ios.ts`, `hapticsMoments.android.ts`) | One set of moments, each platform's own style.                                                                                                                        |
| `launch`       | `127.0.0.1` on the simulator; the device, the engine, the architecture                                        | `10.0.2.2` on the emulator; the device, the engine, the architecture                                                              | An iOS simulator, an Android emulator or an iPhone, as the test chooses                    | `expo-device`, with the address per platform (`launch.ios.ts`, `launch.android.ts`)                         | The simulators reach the Mac differently; nothing else differs.                                                                                                       |
| `appearance`   | The system's light or dark setting                                                                            | The system's light or dark setting                                                                                                | Set by the test                                                                            | Own code over React Native's `Appearance`                                                                   | Part of React Native; no library needed.                                                                                                                              |
| `reduceMotion` | Reduce Motion                                                                                                 | Remove animations                                                                                                                 | Set by the test                                                                            | Own code over React Native's `AccessibilityInfo`                                                            | Part of React Native; no library needed.                                                                                                                              |
| `appState`     | Foreground or background                                                                                      | Foreground or background                                                                                                          | Set by the test                                                                            | Own code over React Native's `AppState`                                                                     | Part of React Native; no library needed.                                                                                                                              |
| `network`      | Connection and its kind                                                                                       | Connection and its kind                                                                                                           | Set by the test                                                                            | `expo-network`                                                                                              | One maintained library covers both.                                                                                                                                   |

Notification actions are part of `push` (categories with actions). Power state is the Mac's
concern in this gate (the daemon keeps the Mac awake), not the phone's.

## What each platform does its own way

People expect their phone's own conventions. The app keeps them, and no screen asks which
platform it runs on: a convention is either the platform's own component, or a value a screen
hands on from `launch.info().conventions` without looking at it.

| Convention | iOS | Android | How the app gets it |
| --- | --- | --- | --- |
| Going back | A swipe from the left edge, or anywhere across the screen; the header's arrow | The system's back gesture or button; the header's arrow | The native stack of `react-native-screens`, with gestures on |
| A screen entering | Slides in from the right | Fades up from the bottom | `conventions.screenEnter`, handed to the stack |
| Making room for the keyboard | The screen pads its bottom | The window resizes | `conventions.keyboard`, handed to `Screen` |
| Haptics | The system's feedback generators | The system's view haptics | `haptics.play(moment)`: one set of moments, each platform's own feel |
| What needs the owner | The system's notifications, with Allow and Deny on them | The app's own banner while it is open (push on Android comes with the relay) | `push` where supported, else the banner of `src/notifications` |
| The launch screen | The mark on the theme's background, from a storyboard | Android's own splash: the mark on one colour | `expo-splash-screen`, configured in `app.config.ts`; the door starts as the same picture |
| Switches, the photo picker, the keyboard | The system's | The system's | React Native's and Expo's components |
| Text size | Dynamic Type, up to the largest standard size | Font scale | `Txt`, which every word goes through |
| Light and dark | Follows the system | Follows the system | `appearance`, a `Live` value |
| Less motion | Reduce Motion | Remove animations | `reduceMotion`, a `Live` value: movement becomes a fade |

Sheets and menus are the app's own on both platforms, so they look like Overseer everywhere.
Screenshots of every screen on each platform are in
`docs/verification/evidence/phone/e2e/<platform>/screens/`.

## The rule, and its check

Outside `src/platform/` the lint fails on:

- `Platform.OS`, `Platform.select`, and any other use of `Platform`;
- importing `Platform` from `react-native` (or from `expo-modules-core`);
- importing a file by its platform name (`./thing.ios`, `./thing.android`), also through
  `require` and `import()`;
- a comparison with a platform's name (`=== 'ios'`, `case 'android':`);
- importing a library that sits behind a capability (`expo-secure-store`, `expo-haptics`, …) or
  the React Native modules that do (`Appearance`, `useColorScheme`, `AppState`,
  `AccessibilityInfo`, …).

`npm run check:platform-rule` proves it: it seeds one violation of each kind outside the
platform layer, runs the lint and asserts that it fails on every one; seeds the same code
inside the platform layer and asserts that it is allowed; removes the files; runs the lint
again and asserts that it passes.

## Using it

```tsx
import { useCapabilities, useLive } from '@/platform';

function PairButton() {
  const { haptics, network } = useCapabilities();
  const { connected } = useLive(network);
  return <Button disabled={!connected} onPress={() => haptics.play('confirm')} />;
}
```

```ts
type PairingSecrets = { devicePrivateKey: string; macPublicKey: string };
const secrets = capabilities.secretStore.scope<PairingSecrets>('pairing');
await secrets.set('devicePrivateKey', encoded);
```

In a test:

```tsx
import { createFakePlatform } from '@/platform/fake';

const { capabilities, fakes } = createFakePlatform({ appearance: 'light' });
await render(<Screen />, { wrapper: providerFor(capabilities) });
await act(() => fakes.appearance.set('dark'));
```

## Adding a capability

1. Write its interface in `capabilities/<name>.ts` and add it to `Capabilities` and
   `CAPABILITY_NAMES` in `capabilities/index.ts` (the compiler checks both).
2. Implement it in `native/`. Use a maintained cross-platform library where one covers it well;
   split into `.ios.ts` and `.android.ts` only where behaviour truly differs.
3. Write its fake in `fake/` and its tests in `__tests__/`.
4. Add its row to the table above, with the choice and the reason.
