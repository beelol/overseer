# The phone's scenario runs: what is here

- `result-ios.json`: the record of the overnight run on the iOS simulator (2026-09-27, 06:23 to 07:21 UTC, release build, real daemon). Passed: pair, agents, conversation, send, review, reject, new, accounts, notifications, update, reopen, off-and-on, manual-address, away, busy. Failed: permission (the flow's last step), push (Maestro cannot see the system's banner), stop-all, queued, unreachable, tour, watch-only, revoke.
- `ios.log`, `ios/`: a later run (12:18 PDT) that was stopped after its second scenario; it rewrote the log and the screenshots of the overnight run, which are lost. What is left: `ios/paired.png`, `ios/agents.png`, `ios/screens/notifications-the-reason-first.png`, `ios/screens/the-system-asks-about-notifications.png`, and `ios/notification-shown.png`, taken by hand after a notification of the daemon's shape was delivered with `xcrun simctl push`.
- `android.log`, `result-android.json`: a run that could not start its lab because another checkout's run held the port. The overnight Android run's log was rewritten by it and is lost.
- `build.log`: the daemon's build for the runs.
- `measure-*.json`: absent; the measurement runs only after every scenario passes.
