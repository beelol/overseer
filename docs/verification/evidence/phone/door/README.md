# The door, recorded

Release builds from a clean clone of the branch (commit 62d8fc0), on the iPhone 17 Pro simulator (iOS 26.5) and the Android virtual device Overseer_API_35 (API 35, without a window), recorded by `phone/e2e/door.mjs` on 2026-09-27. The door opens in about 1 s: the owner found the first 600 ms too fast, and AC-136 on `main` says 1,000 ms within 60 ms.

For each platform:

- `<platform>-cold-start-dark.mp4`, `-light.mp4`: a cold start in each theme, from the home screen to the agents list.
- `<platform>-cold-start-reduce-motion.mp4`: a cold start with Reduce Motion on (iOS's setting; on Android the animation scales at 0). The door fades instead of splitting.
- `<platform>-return-from-background.mp4`: back from the home screen; the app is not launched again and no door shows.
- `<platform>-<recording>-frames.png`: every frame at 30 a second from the launch, four seconds, left to right. The recorder keeps a frame only when the screen changes, so it holds fewer frames than the display drew; the app's own count is in the timings.
- `<platform>-<recording>-opening.png`: the frame most unlike both the closed door and the list, mid-opening (mid-fade with Reduce Motion), at full width.
- `<platform>-timings.txt`: the app's own record of each launch: the marks, the opening's length, the frames it drew and dropped on the UI thread, and the longest frame.

What the recordings show, as the app timed them:

| Recording | iOS simulator | Android emulator |
|---|---|---|
| Dark | 1,022 ms, 61 frames, 0 dropped | 1,058 ms, 57 frames, 4 dropped |
| Light | 1,011 ms, 59 frames, 0 dropped | 1,074 ms, 54 frames, 7 dropped |
| Reduce Motion | a 209 ms fade, 12 frames, 0 dropped | a 240 ms fade, 10 frames, 2 dropped |
| Back from the background | no launch, no door | no launch, no door |

The emulator drew these while recording its own screen and while the Mac started the suites; its 20 measured launches are in `e2e/measure-android.log`. The display budget is the iPhone's (AC-135).

Earlier: `ios-debug-light-opening-frames.png` and `ios-debug-dark-opening-frames.png` are the first recordings, of debug builds, from before the door waited for the first screen to settle.
