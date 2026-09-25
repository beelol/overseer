# RFC: Native Overseer notifications on macOS

Status: proposed (owner request, 2026-09-25). Tracked by
[AC-52](../overseer-rfc.md#gate-i--future-owner-requests-2026-09-25) in the main RFC. Builds on
[AC-45](../verification/AC-45.md) (visible background agents), which is verified.

## Problem

When the last VS Code window closes while agents are running, `overseerd` posts a macOS
notification (AC-45). Today it does that with `osascript -e 'display notification …'`. macOS
attributes those notifications to **Script Editor**, the app that owns AppleScript:

- The banner shows Script Editor's icon, not Overseer's.
- Clicking the banner opens Script Editor instead of VS Code or the Overseer view.
- The on/off switch is under *System Settings → Notifications → Script Editor*. Users will not
  look for Overseer's notifications there, and turning Script Editor off silently hides them.

It works (the owner saw the banner on 2026-09-25), but it looks broken and is hard to control.

## Goal

Notifications that are clearly Overseer's:

1. The banner shows **Overseer** as the app name, with Overseer's icon.
2. Clicking it opens VS Code at the Overseer view (the reopened window lists the running agents,
   as it does today), or does nothing harmful if VS Code is not installed at the known path.
3. The switch appears as **Overseer** in *System Settings → Notifications*.
4. macOS asks once for permission. If notifications are denied or unavailable, Overseer falls
   back to the current `osascript` path, and the reopen message in VS Code (AC-45) still appears.

## Design

Ship a tiny notifier app inside the extension, next to the daemon binary:

```
extension/bin/Overseer Notifier.app/
  Contents/Info.plist        CFBundleIdentifier = com.beelol.overseer.notifier
                             CFBundleName = Overseer, LSUIElement = true (no Dock icon)
  Contents/MacOS/notifier    Swift, ~100 lines
  Contents/Resources/AppIcon.icns   Overseer's eye icon
```

- **Posting.** `overseerd` runs `notifier --title T --body B --open <url>` instead of `osascript`.
  The helper uses `UNUserNotificationCenter`: it requests authorization on first use, posts the
  notification with `userInfo.open = <url>`, then exits (or stays briefly for delivery).
- **Clicking.** The click relaunches the helper, which reads `userInfo.open` and opens
  `vscode://beelol.overseer/open-center`. The extension registers a URI handler that runs
  **Open Overseer View**.
- **Selection order in the daemon.** Use `OVERSEER_NOTIFY_COMMAND` (tests) if set; otherwise the
  bundled helper if it exists and last reported permission; otherwise `osascript` as today. Every
  notice records `delivered_via` (for example `overseer-notifier (ok)`, `overseer-notifier
  (denied; fell back to osascript)`).
- **Build.** `extension/scripts/package.js` compiles the helper with `swiftc` (Xcode command-line
  tools) and assembles the bundle. It is ad-hoc signed (`codesign -s -`). A universal binary
  covers arm64 and x86_64.

## Risks and choices

- **Signing.** An ad-hoc-signed, un-notarized helper can post notifications after the user
  allows them. Gatekeeper may still warn once when it first launches from a downloaded VSIX.
  Developer ID signing and notarization remove the warning and belong with release packaging.
  They are not required for this criterion.
- **Permission prompt.** macOS shows "Overseer would like to send you notifications" the first
  time. The prompt appears when the first background notice is posted, which may be when nobody
  is looking. A **Test notification** command (see Acceptance) lets the user grant it up front.
- **No third-party tools.** `terminal-notifier` and similar would add an install step and still
  carry another app's name, so they are not used.
- **Linux** (AC-41) keeps `notify-send`, which already shows the sending program's name.

## Acceptance

AC-52 in the main RFC is the acceptance criterion. Its Verify clause covers:

- an Overseer-branded banner (name and icon) when the last window closes with agents running,
  seen by the owner or captured in a screenshot;
- clicking the banner opens VS Code at the Overseer view;
- "Overseer" listed in System Settings → Notifications with its own switch;
- the `osascript` fallback when notifications are denied, recorded in `delivered_via`;
- an **Overseer: Test Notification** command that posts a sample banner, so permission can be
  granted up front.
