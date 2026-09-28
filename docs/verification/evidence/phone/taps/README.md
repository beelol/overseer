# Lost taps on the Android emulator: the cause, and the fix (AC-121, AC-126)

In full scenario runs on the Android emulator, a tap now and then went unanswered: the agents menu as a new agent's row arrived, and a hunk's Accept or Reject just after the file screen opened. Maestro saw the screen change and nothing happened. For a night the flows tapped again when that happened; then two experiments looked for the cause, because a retry in a test flow would hide a fault in the app.

## The experiments

Both run only when named, on the Android emulator, against the lab daemon (`phone/e2e/scenarios.mjs`, `tap-timing` and `tap-open`). The taps are sent by adb itself (`adb shell input tap`), not by Maestro. After each tap, the Mac is asked whether the hunk was marked and what the phone sent (`review.accept`, `review.unaccept` or nothing).

- `tap-timing`: Maestro opens the file (the app started anew each time), then adb taps Accept at once, 1.2 s later or 3 s later.
- `tap-open`: adb opens the file from the changes list and taps Accept 300, 600, 1,000 or 2,000 ms later, ten times each, with no Maestro between. A screenshot is kept of each lost tap.

## Before the fix (the app from a673dee)

- `before-fix-tap-timing.log`: 12 taps at once, 6 lost; 5 taps 1.2 s later, 5 lost (stopped there for the second experiment). Each try started the app again, and its tap came about 25 s later.
- `before-fix-tap-open.log`: 40 taps, 1 lost, 600 ms after the file opened; the phone sent nothing for it.
- `before-fix-lost-600ms.png`: the screen just after that tap. A *Reconnecting…* line has appeared at the top and moved everything down one row, so the tap landed on the line above Accept.
- `before-fix-gateway.log`: the lab gateway's log meanwhile: the phone's session was closed 20 s after each connection, *a frame that did not decrypt*, and the phone connected again a second later.

The cause: React Native's WebSocket on Android sends its "ping" as an empty binary message (`WebSocketModule.ping` calls `client.send(ByteString.EMPTY)`), not a ping frame. The session's keepalive called it every 20 s; the gateway read the empty message as a frame that did not decrypt and closed the session. Every scenario still passed, because the session resumes by itself, but each reconnection showed the *Reconnecting…* line for a moment and moved the screen under the finger. `tap-timing`'s taps came close to the first 20 s drop of each new session, the likely reason so many of them were lost.

## The fix (bb7c284b)

- The phone no longer uses the socket's ping on Android (`conventions.socketPing`, from the platform layer); the encrypted keepalive stays on both platforms.
- The gateway ignores an empty binary frame inside a session: a sealed frame is never empty, so nothing that decrypts is skipped, and builds already on phones keep their sessions. Before the handshake an empty frame still closes the connection.
- `cargo test -p overseerd --test gateway ac121_an_empty_frame_in_a_session_keeps_the_session` fails without the gateway change and passes with it.
- The flows tap once again (dec3aaf7): a regression of this kind fails the run.

## After the fix

`after-fix-android.log`: the Android emulator's release app from dec3aaf (the fix, and flows that tap once), `node e2e/run.mjs --platform android --skip-build --only pair,safety,tap-timing,tap-open --write-baseline`:

- `tap-timing`, four tries at each moment: 12 of 12 taps marked, none lost.
- `tap-open`: 40 of 40 taps marked, none lost; the phone sent `review.accept` for each.
- The lab gateway's own count over each experiment: 18 and 19 connections (the app is started anew by the flows), none closed for a frame that did not decrypt.
- The safety scenario passed with its flow tapping the agents menu once. The one other lost tap of the night was of a different kind: the scenario had just made an agent wait for the owner, and Android's own banner for it (over the header, its close button where the menu is) took the tap. The daemon's record routed that moment to the app's banner five seconds before the tap. The flows now put such a banner away before they open the menu, as the owner would (1d285d62).
