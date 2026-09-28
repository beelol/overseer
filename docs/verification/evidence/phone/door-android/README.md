# The Android door's dropped frames (AC-135, AC-136)

Release builds on Overseer_API_35, against a lab overseerd with its own data folder and 15 fixture agents. The phone was paired with `run.mjs --dev <lab> --only pair`. Each run is `node e2e/measure.mjs --platform android --runs 20 --check`, with the load average at its start and end.

| Log | App | Load (1 min, start/end) | Dropped while the door opens | Opening |
|---|---|---|---|---|
| `measure-before-load8.log` | main + measurement (4ac9c3ce) | 7.6 / 8.3 | 18 of 1,214 | 1,026 to 1,166 ms (timed by the JS thread) |
| `measure-after-load10.log` | the fix (929d20dd) | 9.4 / 10.7 | 2 of 1,200 (one launch) | 1,015 to 1,025 ms (timed by the JS thread) |
| `measure-before-load14-25.log` | main + measurement | 14.1 / 25.4 | 44 of 1,194 | 1,037 to 1,143 ms (timed by the JS thread) |
| `measure-after-load27-16.log` | the fix (929d20dd) | 27.5 / 16.0 | 81 of 1,183 | 862 to 1,096 ms (timed by the JS thread) |

- **Load 8 to 10, the fair comparison:** in the before run, every late frame came 50 to 110 ms after the Mac's state arrived during the opening. After the fix, the Mac's state is drawn once the door has gone, and the opening starts on a calm UI thread.
- **Load 14 to 27, the host starved:** the late frames come in both builds, and in the traced launches none of them is the app's work. `traces/after-load24-compositor-starved.txt` shows the RenderThread waiting up to 59 ms in `dequeueBuffer` for the emulator's compositor, with the app's main thread idle. Under that load the JS thread also heard of the opening's start and end late, so its timing read 862 ms and 1,143 ms while the display showed 1,000 ms. The opening is now timed on the UI thread (293c48c1).
- `traces/before-mac-state-mid-opening.txt` is a cold start of the current app (main) under atrace: the first screen's mount (238 views, an 80 ms frame) just before the opening, then the Mac's state drawn in the middle of it.

The traces themselves (about 27 MB each) are not committed. They were taken with `atrace --async_start -a com.beelol.overseer.phone gfx view am wm dalvik sched freq binder_driver input res ss` around `am start`.
