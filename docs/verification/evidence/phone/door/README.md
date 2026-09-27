# The door, as recorded so far

Recordings of the iOS simulator (iPhone 17 Pro, iOS 26.5) in debug builds, read frame by frame with ffmpeg (`fps=30` or `fps=20`, tiled):

- `ios-debug-light-opening-frames.png`: the first door, light theme, 15 frames a second from 5.3 s after launch: the closed door, then the diagonal split with the mark splitting, the pairing screen underneath.
- `ios-debug-dark-lit-and-app.png`: dark theme, 30 frames a second: the door with its gradient, seam light and plating lines lit, then the agents list. In this recording the opening itself was not drawn on screen: the first screen mounted at the same moment and held the animation back. The door now waits for the first screen to settle.
- `ios-debug-dark-opening-frames.png`: dark theme after that change, 30 frames a second: the lit door, then the split (the light along the seam, the plating lines travelling with the halves), then the agents list.
- The app's own record of one launch (debug build, dark): door shown 1,337 ms after the process started, first screen interactive 1,355 ms, opening 629 ms with 36 frames and 0 dropped, longest frame 16.7 ms; after the wait for the first screen: door shown 1,945 ms, opening started 2,063 ms, opening 634 ms, 36 frames, 0 dropped.
- `ios-release-settings-dark.png`, `android-debug-agents-light.png`: two screens from the same evening.

`phone/e2e/door.mjs` records both simulators in release builds (dark, light, Reduce Motion, return from the background) and writes the contact sheets and timings here. It has not run yet: the simulators were taken by the takeover branch's runs.
