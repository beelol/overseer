# Overseer for the phone

The iOS and Android app of Overseer's phone remote, one Expo codebase
([the design](../docs/rfcs/phone-remote.md), Gate N in [the RFC](../docs/overseer-rfc.md)).

One app for both platforms. It lists every agent of the Mac, shows each conversation live,
and controls what VS Code controls: messages, permission requests, review, merge back, pull
requests, new agents, accounts. It talks to the Overseer daemon on the owner's Mac and to
nothing else.

```
app/                  routes (expo-router); the root creates the capabilities and the session
src/theme/            tokens.generated.ts (generated), the themes, useTheme()
src/platform/         one interface per capability, an implementation per platform, fakes
src/session/          the app's one connection: the daemon's state, kept live; conversations
src/door/             the door of a cold start
src/motion/           the one motion system: Tap, Arrive, Pulse, useMotion()
src/ui/               the pieces screens are built from (docs/building-screens.md)
src/screens/          the screens (docs/app-spec.md says what each holds)
src/notifications/    the system's notifications, and the app's own banners where there are none
src/perf/             the app's measurements of its own speed
core/                 the connection library: pairing, the encrypted session, exactly-once requests
model/                the view models, compared with VS Code's own code
integration/          the connection library against a real overseerd
e2e/                  the scenario run on both simulators, and the speed measurements
scripts/              generators, run scripts, and the checks of the platform and token rules
design/               the tokens only a phone has (touch sizes, the door, springs)
assets/               icon and launch images, generated from the mark
app.config.ts         the whole native configuration
```

## Install

Node 24 and npm 11.

```bash
cd phone
npm install
```

The packages beside the app (`core/`, `model/`, …) have their own `npm install` and tests.

## Design tokens

Colours, type scale, spacing, radii and motion values come from the source of the VS Code
themes, `extension/design/tokens.js` (the easing curve from `extension/design/build-themes.js`),
so the phone and VS Code cannot drift.

```bash
npm run tokens          # writes src/theme/tokens.generated.ts
npm run tokens:check    # fails when that file is missing, stale or edited by hand
```

Nothing else in `src/` or `app/` may write a colour, a font size, a space, a radius or a
duration by hand: the lint fails on it. Screens take them from `useTheme()`, which follows the
phone's light or dark setting and changes at once when the setting changes.

## Run on the iOS simulator

Xcode with an iOS simulator runtime, and CocoaPods. No Apple account and no signing.

```bash
npm run ios             # debug build on "iPhone 17 Pro", Metro in the foreground
npm run ios:release     # Release build with its JavaScript inside; no Metro
```

Another simulator: `OVERSEER_IOS_SIMULATOR="iPhone 17" npm run ios`.

## Run on the Android emulator

The Android SDK (platform 36, build tools 36.0.0, NDK 27.1.12297006, an emulator image) and a
JDK 17. Gradle refuses newer JDKs, so JDK 17 goes first on the PATH:

```bash
export ANDROID_HOME=/opt/homebrew/share/android-commandlinetools
export JAVA_HOME=$HOME/.sdkman/candidates/java/17.0.19-tem
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"
```

Overseer has its own virtual device; the run scripts start it when it is not running and never
touch another. It is created once:

```bash
$ANDROID_HOME/cmdline-tools/latest/bin/avdmanager create avd -n Overseer_API_35 \
  -k "system-images;android-35;google_apis;arm64-v8a" -d pixel_7
```

```bash
npm run android           # debug build on Overseer_API_35, Metro in the foreground
npm run android:release   # release build with its JavaScript inside, signed with the debug keystore
```

Another virtual device: `OVERSEER_ANDROID_AVD=Name npm run android`.

## Light and dark

```bash
xcrun simctl ui booted appearance dark      # or light
adb shell cmd uimode night yes              # or no
```

The app follows at once, without restarting.

## Checks

```bash
npm run check
```

runs, in order:

| Script                        | What it checks                                                                                                                   |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `npm run tokens:check`        | The generated tokens equal the VS Code source.                                                                                   |
| `npm run lint`                | Expo's rules, the platform rule and the token rule. No warnings allowed. The packages beside the app get the platform rule only. |
| `npm run typecheck`           | TypeScript, `strict`, including both platforms' files.                                                                           |
| `npm test`                    | Unit tests, on the Mac, with no simulator.                                                                                       |
| `npm run check:platform-rule` | Seeds violations of the platform rule, asserts the lint fails on each, removes them, asserts the lint passes.                    |

## Decisions

**Expo SDK 57** (React Native 0.86, React 19.2), the current release. It builds with Xcode 27
and runs on the iOS 26.5 simulator and on Android 15. The New Architecture and Hermes are the
only architecture and engine this React Native has; the first screen shows both as the
runtime reports them.

**`ios/` and `android/` are not kept in the repository.** They are generated from
`app.config.ts` and the config plugins by `expo prebuild` (continuous native generation), so
the native projects cannot drift from the configuration and an SDK upgrade does not leave
hand edits behind. The run scripts generate a missing native project by themselves. After a
change to `app.config.ts` or to a native dependency, generate them again:

```bash
npm run prebuild        # expo prebuild --clean
```

Native behaviour is changed in `app.config.ts` or in a config plugin, never in `ios/` or
`android/`.

**The run scripts use `xcodebuild`, `gradlew`, `simctl` and `adb` directly**, not
`expo run:ios` and `expo run:android`. Two reasons, both met on this Mac:
`expo run:ios --device "iPhone 17 Pro"` takes the first simulator with that name, including
one whose runtime is no longer installed, and fails to boot it; and both commands look for the
simulator's window with AppleScript, which needs the Mac's Automation permission and otherwise
ends after two minutes with the app installed but never opened (iOS) or opened two minutes
late (Android). The scripts choose an available simulator by name and need no permission. The
build's full output goes to `ios/build/xcodebuild-<configuration>.log` and
`android/build/gradle-<variant>.log`.

**Tests run with `jest-expo`**, not vitest. The theme hook and the screens render React
Native components, and `jest-expo` is the maintained preset that transforms React Native's
sources and mocks the native modules of this exact SDK. vitest cannot load React Native
without a transform pipeline of our own. `core/` has no React Native in it and uses vitest.

**The platform layer** has one typed, generic interface per capability, an implementation per
platform and a fake for tests. The table of capabilities, the rule and its check are in
[src/platform/README.md](src/platform/README.md). The device's capabilities are created in one
place, `app/_layout.tsx`, and reach everything else through `<PlatformProvider>`, exactly as
the fakes reach a test.

**`keyValue` is `expo-sqlite`'s key-value store**, because its reads are synchronous (the first
screen is drawn from the cache before anything asynchronous runs) and Expo maintains it with
the SDK.

**The gesture and animation libraries are named in `package.json`**
(`react-native-gesture-handler`, `react-native-reanimated`, `react-native-worklets`) although
no screen animates yet. expo-router's navigators need them, and unnamed, npm installs their
newest versions, which this SDK has not been validated with. `npx expo install --check`
confirms every dependency is the version the SDK expects.

**`react-dom` is overridden to React's version.** The app has no web target, but packages
under expo-router name `react-dom` as a peer and npm would install a newer one than React.

**The icon and the launch images are drawn from the stand-in mark**
(`extension/media/overseer.svg`) and the tokens by `npm run assets`, because the owner's mark
(AC-142) is not in `docs/design/brand/` yet. The mark is read from one place in
`scripts/gen-assets.mjs`; swapping it is one change. The door (AC-136) replaces the launch
image.

**The notifications library is left out of the Android build** (`expo.autolinking` in
`package.json`). Push reports unsupported on Android in this gate, and the library would
otherwise bring eleven permissions and a boot receiver into an app that uses none of them. It
comes back with Android push.

**Android asks for no permission the app does not use.** The template's storage and overlay
permissions are blocked in `app.config.ts`.

**Changing a native dependency** needs `npm run prebuild`, which also removes the build output
Gradle leaves inside `node_modules` (`scripts/clean-native.mjs`). Without that the next Android
build fails with `ninja: error: ... missing and no known rule to make it`.

**Portrait and landscape, phones only.** Tablets are later work.

## Not in the foundation

- Browsing for the Mac with Bonjour or network service discovery: `discovery` reports
  unsupported with the reason. Manual addresses and the simulators' own addresses work.
- Push on Android: reports unsupported (it needs a Firebase project and comes with the relay).
- A release build talks to the gateway over a plain WebSocket, encrypted at the message
  layer. Android refuses unencrypted connections in release builds unless the app allows
  them; that setting belongs with the transport and is not made here.
- Signing, the real iPhone, TestFlight: the owner's steps in the design.

## The scenario run

One command builds the daemon and both release apps, installs each app anew on its simulator,
starts a real `overseerd` with its own data folder and fixture agents, drives the app with
Maestro through every scenario, asks the daemon what happened, and measures the speed budgets.

```bash
npm run e2e
```

```bash
node e2e/run.mjs --platform ios --skip-build --only pair,send
```

It needs Maestro (`brew tap mobile-dev-inc/tap && brew install maestro`), the booted simulator
and Overseer's own Android virtual device. Logs, screenshots and results go to
`docs/verification/evidence/phone/e2e/`. It never touches the owner's daemon: the lab's daemon
listens on its own ports (47821 and 47822) and is never advertised on the network.

`node e2e/run.mjs --seed-slow 400` holds every start of the app for 400 ms: the budgets must
notice and the run must fail. `node e2e/measure.mjs --platform ios --write-baseline` records a
platform's baseline in `e2e/baselines.json`.

## Checks

```bash
npm run check
```

Tokens and icons current with their sources, the lint (with the platform rule and the token
rule), the types, the unit tests, and the two rules proven with seeded violations.
The packages beside the app: `npm test` in `core/`, `model/` and `integration/`.
