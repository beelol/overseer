# AC-52 live session with the owner (2026-09-25, owner's Mac, macOS 26.6.2)

Hybrid check: the owner looks and clicks; the agent (no screen access) prepares, reads the daemon and records.

1. Agent: installed build is final (`overseerd-darwin-arm64` = release build; `Overseer Notifier.app` signed,
   `com.beelol.overseer.notifier`), no runs active. Notifier permission: `notDetermined`.
2. Owner ran **Overseer: Test Notification** (first build). Daemon log:
   `test notice (overseer-notifier (denied); fell back to osascript (ok))`; permission still `notDetermined`.
   Running the helper directly reproduced macOS's error: `notifications denied: Notifications are not allowed
   for this application` — no prompt. Launching the same app through LaunchServices (`open -n -W`) got it
   registered (status changed to `denied`).
   **Fix** (commit 2b162cd): the daemon now launches the helper with `open -n -W` and reads its outcome from a
   `--result` file. Stale LaunchServices registrations of test copies were unregistered.
3. Owner (quote): "overseer is in there listed as off" — System Settings → Notifications lists **Overseer**.
   (A separate "terminal-notifier" entry is not Overseer's.)
4. Owner ran Test Notification on the fixed build and sent two screenshots:
   - `owner-01-macos-permission-prompt.png`: macOS's permission banner **"“Overseer” Notifications"** with the
     Overseer eye icon.
   - `owner-02-fallback-before-allow.png`: the fallback banner (Script Editor icon) "Overseer notifications are on",
     because permission was not yet granted. Owner (quote): "it showed the apple script thing again and said this
     is how it will look."
   Daemon log: `test notice (overseer-notifier (denied); fell back to osascript (ok))`.
5. Owner (quote): "I allowed it." Notifier permission: `authorized` (read with `notifier --status`).
6. Agent sent the test notice through the owner's daemon at 03:34:48Z (`overseerd ctl daemon.test_notice`):
   `{"delivered_via":"overseer-notifier (ok)"}` — delivered by the Overseer app, no fallback.
