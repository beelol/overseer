# The phone's scenario runs: what is here

Release builds of the app on the iPhone 17 Pro simulator (iOS 26.5) and the Android virtual device Overseer_API_35 (API 35, run without a window), against a real overseerd with its own data folder and fixture agents, driven by Maestro. The run is `npm run e2e` (`phone/e2e/run.mjs`) from a clean clone of the branch: `git clone`, `cd phone && npm ci && npm run prebuild`, then the one command. It builds the daemon and both release apps, runs every scenario on each platform, one platform after the other, and measures the budgets.

- `clean-clone.log`: the clone, its commit and its setup, then the run's own output, with the load average at its start.
- `ios.log`, `android.log`: each platform's log; the commit is on its first line.
- `result-ios-android.json`: every scenario on both platforms with its outcome, its seconds and, where it measures something, its figures (the delay from the Mac's event to the frame, the agents found, the diff's lines, the scroll's frames, the key search). `budgets` is the measurement.
- `measure-<platform>.json`, `measure-<platform>.log`: 20 cold starts with the door and 20 without, each launch's marks, the verdicts and the load average at the start and the end. The last run checks every budget against `phone/e2e/baselines.json` (AC-135: a later run within 10%). iOS's baseline was written by the clean-clone run at dec3aaf; the Android emulator's own, by `baseline-android.log` and `baseline-android.json` (its release app from dec3aaf: pairing, the safety scenario and the tap experiments, all passed, then the 40 launches). The emulator is held to its own baseline; the 1% display budget is the owner's iPhone's.
- The runs of the night before the last one found two faults on the Android emulator, both fixed: the session dropped every 20 s, which lost taps (`../taps/README.md`), and the emulator's door frames were held to the iPhone's budget.
- `<platform>/`: what the scenarios photographed; `<platform>/screens/<theme>-<size>/`: the tour, every screen in dark and light at the smallest and the largest text size; `<platform>/screens/*.png`: single moments (the notification before Allow, queued while away, the Mac unreachable, each confirmation asking once, the safety settings on, the app locked, an image attached, a diff's lines).
- Maestro's own record of each flow (`maestro/<platform>/<flow>/`, with every command, its screenshots and the device's logs) is written by each run but not committed: a full run's is about 500 MB. What each flow did is in the platform's log and the scenario's screenshots.
- `restart/`: the `restart` scenario (the simulator or the emulator rebooted), also in the full run, run once on a lab of its own.
- `seeded-slow-<platform>.log`: `--seed-slow 400 --runs 3`, every start held for 400 ms; the budgets must fail, and do.
- `live/`: `phone/e2e/live.mjs`, one tiny live turn each on Claude Code (Haiku) and Codex (gpt-5.6-luna, low effort) started from the phone and answered, within the owner's paid-turn rules: the record, the log, and the form and the answer for each.
- `build.log`: the daemon's build for the runs.

The lab's state files are not committed: they name folders under `/tmp` and a pairing code that expired.
