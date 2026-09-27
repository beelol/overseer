# Side RFC: phone remote on the same network

Status: proposed by the owner on 2026-09-26. Acceptance criteria: AC-115 to AC-137 and AC-141 (Gate N) in the
[main RFC](../overseer-rfc.md#gate-n--phone-remote-on-the-same-network-added-by-the-owner-2026-09-26).
Prepared goal: [phone-remote-goal.md](phone-remote-goal.md) (not activated).
Builds on the daemon's event replay ([AC-10](../verification/AC-10.md)), the local access boundary
([AC-08](../verification/AC-08.md)), hunk accept and reject ([AC-42](../verification/AC-42.md)) and
the chat with Overseer itself (AC-107, Gate M).

## Why

Agents keep working after the owner walks away from the Mac, and then they wait: for a permission,
an answer, a review. Today the only way to see or answer them is VS Code on that Mac. The owner
wants the phone to be a full remote: see the output of any agent, talk to any agent and to Overseer
itself, and reach every Overseer system, with full control and without losing the session. The
owner's words: *if it's on, you can access any agent through your computer from your phone*.

Existing remote controls for single harnesses drop their session when the phone sleeps or the
network changes. Overseer is built so that cannot lose anything: the daemon owns all state and
already replays its event log from a cursor.

## Decisions from the owner (2026-09-26)

| Topic | Decision |
| --- | --- |
| What the phone talks to | Overseer only (the daemon on the Mac). The phone never talks to Claude, Codex or any harness directly. |
| Where agents run | On the Mac, as today. The Mac must be on. |
| First step | The same network: phone and Mac on one local network. |
| Gateway | Approved: a gateway inside the daemon is the door the phone connects to. |
| Relay | Wanted, later, as its own RFC and gate: a server both sides dial out to, carrying encrypted messages, for use away from home. Not approached in this gate. |
| Control | Full control of every agent and every Overseer system. |
| Sessions | No lost sessions ("no disconnect"). |
| Platforms | An iOS app, tested on the owner's iPhone. Android tested on the emulator only for now. |
| Apple Developer Program | The owner has a membership: device installs, TestFlight and push notifications are available. |
| App stack | The best option that is not slow. Expo is acceptable. Rust is not forced: it is used only where it is needed. The owner likes native views. |
| Platform behaviour | Separated per platform behind generic interfaces. Cross-platform libraries are used where they fit. |
| Speed | The app is hyper fast. |
| Motion | Cool animations, with a signature launch: a sci-fi gradient door carrying a grayscale logo that splits open diagonally when the app is ready. |
| The door | On a cold start only. |
| Themes | Light and dark, following the phone's system setting. The app looks the same as Overseer Light and Overseer Dark in VS Code. |
| Gate M | Ignored for now. No third theme on the phone. The chat with Overseer on the phone (AC-128) waits for AC-107 and does not hold up the rest. |
| Default scope | Full control (confirmed). |
| Pair once | After the first pairing the owner never pairs or signs in again. The mechanism is the implementing agent's choice. |
| On and off | Phone access is turned on and off on the desktop. |
| Notifications | Wanted, and they can be turned on and off. |
| Simulators first | Build and verify on the iOS simulator and the Android emulator now. The real iPhone comes later, with the steps for the owner listed at the end. |
| Apple assets | The Apple developer assets already on this Mac belong to another project of the owner's. They are not read, used or changed. Overseer gets its own. |
| Voice mode | Not wanted. |
| The Mac | Stays on. |
| Everything else | Left to the implementing agent: *you pick the best option*. |

## Proposed defaults, distinguished from the decisions above

On 2026-09-26 the owner left the remaining choices to the implementing agent. These are the
choices made. The owner can change any of them; a change is a recorded revision of this RFC.

- **Expo with React Native for the app**, in `phone/`: it draws native views, which the owner
  likes, and it can share the extension's design tokens and conversation code. It is the stack of
  the simulator milestone. The speed budget is measured on the owner's iPhone as the first device
  step; **Flutter is the fallback** if Expo misses it (see [The app stack](#the-app-stack)).
- **Pairing with keys, not a cloud account.** A cloud link would need a server, an account and a
  sign-in, which is exactly what the owner does not want to repeat. Keys are exchanged once and
  kept (see [Pair once](#pair-once)).
- **No app lock by default.** The phone's own lock protects it. An app lock is a setting.
- **Rust on the phone only for the encrypted session**, and only if the spike shows a clean
  binding. Everything else is written in the app's own language.
- **Message-layer encryption** with an established handshake (Noise IK proposed), so the same
  session can later cross a relay that sees only ciphertext. Chosen in AC-115.
- **Three things stay on the Mac**: pairing a device, managing devices, and stopping the daemon.
  A phone that could do these could lock itself out or let another device in.
- **Review on the phone** reads diffs and accepts or rejects hunks. Editing files on the phone is
  not in this gate.
- **New agents start in repositories Overseer already knows.** Typing an arbitrary path on the
  phone is not in this gate.
- **Notifications carry no prompt, code or file content** by default.
- **Push on iOS only** in this gate, sent by the daemon straight to Apple with Overseer's own key.
  Android gets in-app notifications; Android push needs a Firebase project and comes with the relay.
- **Private addresses only**: the gateway accepts connections from private and link-local ranges.

## Vocabulary

| Term | Meaning |
| --- | --- |
| Gateway | The listener inside `overseerd` that phones connect to. Off by default. |
| Phone access | The daemon setting that turns the gateway on and off. |
| Pairing | The one-time introduction of a phone to a Mac, started on the Mac, that exchanges keys. |
| Device | A paired phone: a name, a platform, a public key, a scope, and when it was last seen. |
| Scope | What a device may do: *Full control* or *Watch only*. |
| Session | One authenticated, encrypted connection from a device to the gateway. |
| Cursor | The sequence number of the last event the phone applied. It resumes from there. |
| Request id | A unique id the phone puts on every changing request, so a retry never acts twice. |
| Relay | A future server that passes encrypted messages when phone and Mac are on different networks. Not in this gate. |

## Shape

```
  iPhone / Android app                          Mac
  ┌───────────────────┐    same network    ┌──────────────────────────────┐
  │ Overseer (phone)  │ ◄────────────────► │ overseerd                    │
  │  cursor, cache    │  encrypted session │  ├─ gateway (off by default) │
  │  device key       │                    │  ├─ Unix socket (VS Code,    │
  └───────────────────┘                    │  │   TUI, ctl) — unchanged   │
                                           │  ├─ state, events, runs      │
                                           │  └─ harnesses (Claude, …)    │
                                           └──────────────────────────────┘
```

The phone is one more client of the daemon, like VS Code and the terminal UI. It keeps no state of
its own beyond a cache, its cursor, unsent messages and its key. Everything it shows comes from the
daemon; everything it does is a daemon request.

## The gateway

- **A second entrance, not a wider first one.** The Unix socket, its owner-only permissions and its
  peer check (AC-08) do not change. The gateway is a separate listener with its own authentication.
  This is a recorded revision of the main RFC's local-only default.
- **Switched on the desktop.** Phone access is off until the owner turns it on, and it is turned
  on and off on the Mac only: a command and a status bar item in VS Code, a key in the terminal
  UI, and `overseerd ctl`. The Mac shows whether it is on and how many phones are connected. A
  phone cannot change it. The daemon persists and enforces the setting, so it holds with VS Code
  closed.
- **Off is said, not guessed.** Turning it off tells the connected phones first, then closes the
  listener and every session. The phone says *Phone access is off on the Mac*, which is different
  from *Mac unreachable*. Turning it on again lets paired phones reconnect by themselves.
- **Transport.** WebSocket over TCP on one port (default 47810, configurable), on the Mac's network
  interfaces and on loopback (the iOS simulator and the Android emulator connect through loopback).
- **Same protocol.** Inside a session the phone speaks the daemon's versioned JSON protocol: the
  same methods and the same `events.subscribe` replay VS Code uses. It says `hello` as client
  `phone` with its device id.
- **Every method is classified**: *read*, *control*, or *Mac only*. A method without a class fails
  the test suite, so a method added later cannot become reachable from a phone by accident.
  *Watch only* devices get *read*; *Full control* devices get *read* and *control*.
- **Limits.** The protocol's request size limit applies after decryption. Failed handshakes are
  rate-limited per address and logged. Nothing answers before a session is authenticated.

## Pairing and devices

1. On the Mac: **Overseer: Pair a Phone**. VS Code shows a QR code and the same content as a short
   typed code. It carries the gateway's public key fingerprint, the addresses to try, and a secret
   that works once and for two minutes.
2. On the phone: scan the code or type it. The simulator and the emulator have no camera, so typing
   is a first-class path.
3. The phone connects, proves it knows the secret, and sends its own new public key and name.
4. The Mac shows *Pair "Bilal's iPhone"?* The owner confirms on the Mac.
5. The daemon stores the device. The phone stores the Mac's key and its own private key in the
   system keystore (Keychain on iOS, Keystore on Android). Keys never leave the device.

### Pair once

Pairing happens one time. After it, opening the app never asks for anything: no pairing, no
sign-in, no confirmation. The app has no account, no password and no sign-in screen. It reconnects
by itself after the app or the phone restarts, the app is updated, the Mac or the daemon restarts
or is updated, the Mac's address changes, phone access is turned off and on again, and a month
without use. Pairing ends only when the owner revokes the device on the Mac or removes the app.

The device's keys are its identity on every route. When the relay is added later, the same keys
work through it, so nothing is paired again.

### Refusals and the device list

A wrong, expired or reused secret pairs nothing. Five failures close pairing until it is started
again. The **Devices** list on the Mac shows each device's name, platform, scope, paired time, last
seen time and address, with **Revoke**. Revoking ends the device's session at once; its key never
works again.

## Encryption

Every session is mutually authenticated with the keys exchanged at pairing and encrypted at the
message layer, under the WebSocket. Properties required, whatever construction AC-115 selects:

- both sides prove their identity; an unknown device and an impostor Mac are both refused;
- forward secrecy per session; a tampered, replayed or reordered frame ends the session;
- nothing of the protocol is readable on the wire, including method names and prompts;
- the session layer does not depend on the transport, so a relay can carry it later unchanged.

The proposal is the Noise framework's IK pattern (the phone knows the Mac's key from pairing), with
the `snow` crate on the daemon and a vetted implementation on the phone. TLS with a pinned
self-signed certificate was considered: it protects a direct connection equally well, but a relay
would end the TLS connection and could read everything, so the later gate would need a second
scheme. One scheme for both is simpler to verify.

## Finding the Mac

While phone access is on, the daemon advertises `_overseer._tcp` over Bonjour. The phone finds its
Mac by key, never by name or address alone, so a changed address needs no new pairing and a machine
with the same name and another key is refused. A manual address always works; the Android emulator
reaches the host at `10.0.2.2` and cannot see Bonjour.

iOS asks once for permission to find devices on the local network. The app explains why before the
system prompt appears, and says what to do if it was denied.

## Never losing the session

A phone's connection will drop: iOS suspends apps in the background, phones lock, Wi-Fi changes.
The session state is never in the connection.

- **Resume from the cursor.** On every reconnect the phone subscribes after its cursor. The daemon
  replays what was missed, then continues live. Each event is applied exactly once, in order.
- **Truncated history.** If the daemon no longer has events after the cursor, it says so; the phone
  reloads state and tells the user.
- **Cache first.** The app opens on its cached state at once, marked with its age, and updates when
  the session is back. It never shows stale state as live.
- **Exactly-once requests.** Every changing request carries a request id. The daemon stores the
  outcome per device and id for at least 24 hours and answers a retry with the stored outcome. A
  message typed without a connection is kept on the phone, shown as queued, and sent once.
- **An awake Mac.** While phone access is on and a run is active or waiting for the owner, the
  daemon holds a power assertion that prevents idle sleep, and releases it afterwards. A closed lid
  on battery sleeps anyway; the README says so. When the Mac cannot be reached the phone says *Mac
  unreachable* with the last contact time.

## Several surfaces at once

The phone, VS Code and the terminal UI can all be connected at the same time. They share one
daemon and one event stream, so what is done on one shows on the others at once. When two
surfaces answer the same permission request, the daemon takes the first answer and refuses the
second with the first one's outcome. No surface ever shows a request as open after it was answered.

## What the phone can do

| Area | On the phone | Daemon work needed |
| --- | --- | --- |
| Agents | The side bar's list: hierarchy, status, logos, Needs you first, filter, search | None |
| Conversation | Live chat for any agent, to the same content as VS Code | None |
| Control | New agent, follow-up with per-turn options, images from the phone, queue, stop, Allow and Deny, pin, archive | Request ids |
| Review | Changed files, comparison choice, diffs, hunk accept and reject, live refresh | File and diff methods; reviewed marks move from the extension to the daemon |
| Accounts | Sign-in state, plan, usage and limits; device-code sign-in | None for reading |
| Merge back, clean up | The same steps as VS Code, with what would be lost named first | None |
| Pull request | Opened by the daemon with the Mac's Git and GitHub credentials | PR creation moves from the extension to the daemon (`gh`) |
| Talk to Overseer | The same chat as AC-107, continued across phone and VS Code | The Overseer chat's session lives in the daemon |
| Notifications | Needs-you push on iOS; Allow and Deny from the notification | Push sender, device tokens |
| Mac only | Pair a phone, manage devices, stop the daemon, sign-ins that need a terminal on the Mac | — |

Today three things live in the extension rather than the daemon: reading file contents for review,
the reviewed marks on hunks, and opening a pull request with VS Code's GitHub sign-in. A phone
cannot reach the extension, so each moves behind a daemon method. VS Code then uses the same
methods, which keeps both surfaces in agreement.

A **capability table** in the README lists every daemon method with its phone status: available,
Mac only with the reason, or not yet. It is generated from the gateway's method classes.

## Notifications

The daemon sends needs-you notifications (a permission request, a question, a failure, a finished
turn) to paired iPhones. It talks to Apple's push service directly, with Overseer's own push key
kept in the macOS Keychain. There is no Overseer server.

- **Switches.** On the phone: one switch for all notifications and one per kind. On the Mac: one
  switch for every phone. Off means nothing is sent, not sent and hidden.
- The system's permission is asked once, after pairing, with the reason given first.

- The notification names the agent and the kind of event. It carries no prompt, code or file
  content unless the owner turns that on.
- Tapping opens that agent. A permission request offers **Allow** and **Deny** on the notification,
  after the phone is unlocked.
- No push is sent while VS Code is focused on that agent.
- A push arrives wherever the phone is. Away from the local network the app opens, says the Mac is
  unreachable from this network, and shows its cache. Acting from there is the relay gate's work.

Distributing the app to other people needs a push service that holds the key, because a key cannot
ship inside an app. That service belongs with the relay.

## Security and privacy

A paired phone with full control can start agents that run code on the Mac. The gateway is treated
as the most sensitive surface in Overseer.

- Pairing needs the Mac's screen and a confirmation on the Mac.
- No credential, token, key or credential file content is ever sent to a phone. Sign-in on the
  phone uses the provider's device-code flow in the phone's own browser.
- Destructive actions from the phone name what will be lost and ask for one confirmation.
- The app does not lock itself by default; the phone's own lock is its protection. An app lock
  and a device unlock before destructive actions are settings, both off by default.
- Every command from a phone is recorded as an event with the device as its source.
- The daemon's secret redaction applies to everything sent, as it does for VS Code.
- The gateway change gets a security review and a fuzz test of its handshake and frame parser.

## Reuse

[Happy](https://github.com/slopus/happy) (MIT) is a phone client for Claude Code and Codex. Its
design documents and dependency list were read on 2026-09-26; its source has not been inspected
line by line.

| Part of Happy | Decision | Reason |
| --- | --- | --- |
| Computer-side wrapper and session manager | Not used | It wraps the harnesses itself. Overseer's daemon already owns runs, accounts, worktrees and events. |
| Server (Postgres, Socket.IO) | Not used | This gate has no server. The relay gate will reassess. |
| Wire protocol | Not used | Overseer has its own versioned protocol with replay. |
| Chat list approach (inverted list, scroll anchoring) | Candidate | A stable streaming chat on a phone is hard; their notes record what works. |
| Diff rendering approach (highlighting off the main thread) | Candidate | Keeps scrolling smooth in large diffs. |
| Encrypted blob layout | Reference only | Useful when the relay stores messages; a live session uses a handshake instead. |
| Voice, in-app purchases, analytics | Not used | Out of scope. |

Happy is written for React Native. If AC-115 chooses Flutter, its approaches can inform the
design but its code cannot be adopted.

AC-115 inspects the candidate code at a recorded revision and writes the decision in
[source-assessment.md](../source-assessment.md). Anything adopted keeps its license and notice.

## The app stack

The owner's rule is the best option that is not slow. The candidates differ in what actually runs
on the phone.

| | Flutter | Expo (React Native) | SwiftUI and Jetpack Compose |
| --- | --- | --- | --- |
| What runs on the phone | Dart, compiled ahead of time to machine code | JavaScript, shipped inside the app as precompiled bytecode and run by the Hermes engine | Swift and Kotlin, compiled |
| What draws the screens | Flutter's own GPU renderer | The platform's native views | The platform's native views |
| Animation | Every frame comes from one engine; custom shapes and shaders are ordinary work | Smooth when the animation runs outside JavaScript; busy JavaScript can still delay what it drives | The reference |
| Codebases | One | One | Two |
| Reuse from Overseer | None directly | The extension's JavaScript conversation and Markdown code | None |
| Platform features | Plugins, with native code where none fits | Expo's modules | First party |
| Tests without a device | Screen and screenshot tests run on the Mac | Logic tests on the Mac; screens need a simulator | Simulator |

React Native is not a web page in an app: its screens are real native views, so text selection,
scrolling, the keyboard and accessibility behave exactly as the platform's own. Its logic is
JavaScript at run time. This app does little computing on the phone (the Mac does the work and
the phone displays it), so the risk is not raw speed but busy JavaScript delaying what it drives.
Flutter is compiled and draws every frame itself, which makes animation consistent, but its views
are drawn to resemble the platform's rather than being them. Two native apps would be fastest and
would double every screen.

The rule: Expo is measured first and chosen when it meets the speed budget (AC-135) on the
owner's iPhone, because the owner likes native views. Flutter is measured and chosen if Expo
misses. The numbers decide, not opinion.

## App architecture

```
  screens ── state ── protocol client ── encrypted session ── transport
     │                      │
     └── platform layer ────┘        generated protocol types
         (one generic interface per capability;
          an implementation per platform; a fake for tests)
```

- **Screens and shared code never ask which platform they run on.** They use the platform layer's
  interfaces. A build check fails on a platform test anywhere else.
- **One generic interface per capability**, for example a store typed by what it holds and a
  capability typed by its configuration and result. Each has an iOS implementation, an Android
  implementation and a fake.
- **Libraries first.** A maintained cross-platform library sits behind the interface wherever one
  covers the capability well. Own native code is written only where none does.
- **Honest gaps.** A capability a platform lacks reports unsupported with the reason.
- **Typed protocol.** Requests, results and events are generated from one protocol description,
  which is also the source of the gateway's method classes. A method cannot change on one side only.

| Capability | iOS | Android | Shared |
| --- | --- | --- | --- |
| Key storage | Keychain | Keystore | One library |
| Discovery | Bonjour, with the local network permission | Network service discovery; none on the emulator | A manual address |
| Push | Apple's push service | In-app only in this gate (reports unsupported) | — |
| Device unlock | Face ID, Touch ID, passcode | Fingerprint, face, PIN | One library |
| Camera for pairing | Camera | Camera | A typed code on simulators |
| Launch screen | The full closed door | A centered logo on the door's base color | The app's door takes over from both |
| Back | Swipe from the edge | The system back gesture | — |
| Haptics | The system's feedback styles | Vibration effects | One set of moments |
| Notification actions | Allow and Deny after unlock | In-app only in this gate | — |

Android 12 and later allow only a centered icon on one color as a launch screen, so the two
platforms start differently and meet at the same door.

## Look

The app has two themes, light and dark. They follow the phone's system setting and change at once
when it changes. They look the same as Overseer Light and Overseer Dark in VS Code, because both
are generated from one source, `extension/design/tokens.js`: colors, type scale, spacing, radii
and motion values. A check fails when a phone token differs from that source. Provider logos are
the same files. What differs on purpose is what a phone needs: larger touch targets, the system
text size, and each platform's own conventions.

## Speed

| Budget | The owner's iPhone | Android emulator |
| --- | --- | --- |
| The closed door on screen | First frame | First frame |
| Cached agents list ready to use after launch | 1 s at p95 | 2 s at p95 |
| A visible response to a tap | 100 ms | Its own baseline |
| A sent message visible in the chat | 50 ms | Its own baseline |
| Dropped frames in a 5,000-item conversation during a stream | At most 1% at the display's rate | Its own baseline |
| Animation while the app's logic is busy for 500 ms | No dropped frame | Its own baseline |
| A streamed line from the Mac to the screen (AC-124) | 500 ms at p95 | 500 ms at p95 |

The emulator's frame rates say little about a real phone, so it records a baseline and later runs
must stay within 10% of it. How the app meets the budget:

- **Open on the cache.** The first screen is drawn from stored state, before the connection exists.
- **Show before confirming.** A sent message appears at once, marked as sending.
- **Apply events in batches**, once per frame, so a fast stream never redraws more than the screen
  can show.
- **Build only what is visible** in lists, conversations and diffs.
- **Keep motion independent of logic**, so a busy moment cannot stutter an animation.
- **Measure on every run.** The budgets are part of the regression run; a slow change fails.

## The door and motion

```
   closed                      opening                     open
  ┌───────────────┐          ┌───────────────┐          ┌───────────────┐
  │░░░░░░░░░░░╱▓▓▓│          │░░░░░░░╱     ╱▓▓│          │               │
  │░░░░░░(◐╱◑)▓▓▓▓│    ──►   │░░░(◐╱     ╱◑)▓▓│    ──►   │    the app    │
  │░░░╱▓▓▓▓▓▓▓▓▓▓▓│          │╱     ╱▓▓▓▓▓▓▓▓▓│          │               │
  └───────────────┘          └───────────────┘          └───────────────┘
```

- **Closed.** A sci-fi gradient built from the active theme's colors fills the screen: a dark
  door in dark mode and a light door in light mode, following the phone's system setting. A
  grayscale Overseer logo (from `extension/media/overseer.svg`) sits across a diagonal seam.
- **Waiting.** A slow light travels along the seam, so the door is clearly alive.
- **Opening.** When the first screen is drawn, the door splits along the seam. The halves slide
  apart and the logo splits with them. The app is already in place underneath.
- **No jump.** The system's launch screen shows the same closed door in the same place and mode.
- **Never slower.** The door opens as soon as the first screen is ready from the cache. It does
  not wait for the connection and has no minimum time on screen. Launch to a usable list is the
  same with the door turned off.
- **Cold start only** (the owner's decision): returning from the background shows no door.
- **About 600 ms**, interruptible by a touch. With Reduce Motion on it fades.

The rest of the app moves with the same care. One motion system, with durations, easing and
springs as tokens, drives every transition. Motion explains where a thing came from or what
changed. It can be interrupted, follows the finger, and is replaced by fades under Reduce Motion.
The owner marks the door and each transition on a review page until they look right.

## Working alongside the other gates

Other gates are being built while this one is planned. Where they meet:

- **Phases 1 to 3** add a new module to the daemon and change little existing code. They can be
  built while anything else is in flight.
- **Phase 4** moves file reading, reviewed marks and pull request creation behind daemon methods.
  Gate M reworks the review in VS Code (AC-99). The daemon methods are the contract between them:
  this gate defines them, and whichever lands second builds on the first.
- **Continuity (Gate L)** adds a connection state. The phone shows it when it exists.
- **The terminal UI** does merge back, pull requests and cleanup in its own code today. It can
  use the daemon methods after phase 4; this gate does not require it.
- **Gate M is ignored for now** (the owner's decision). Only AC-128 depends on it.

## Testing

- **Daemon:** gateway protocol tests in `cargo test` with a real daemon binary, as today.
- **iOS simulator:** shares the Mac's network; connects through loopback. No camera. A
  notification with the daemon's exact payload is delivered with `xcrun simctl push`.
- **Android emulator:** connects to `10.0.2.2`. No Bonjour, no camera.
- **The owner's iPhone, after the device steps:** Bonjour, the local network permission, the
  camera, push through Apple's service, and real speed. Only a real device can verify these.
- **Phone scenarios** run against a real daemon with fixture harnesses, from one command.
- **Speed** is measured on release builds by the same run that checks everything else: baselines
  on the simulator and the emulator now, the budget itself on the owner's iPhone later.
- **Live turns** follow the owner's paid-turn rules: tiny prompts, one attempt per step.

## Limits and out of scope

- **Away from home.** Different networks need the relay (its own RFC and gate). A VPN that puts
  phone and Mac on one private network may carry the same connection; it is untested and not
  claimed by this gate.
- **The Mac must be on.** Nothing runs in the cloud. A sleeping or shut Mac is unreachable.
- **Voice mode** is not wanted (the owner's decision). The keyboard's own dictation works in
  every text box.
- **Editing files on the phone**, typing an arbitrary repository path, Android push, tablets,
  watch apps, widgets and App Store distribution are later work.
- **Linux hosts** belong to AC-41. The gateway's platform-specific parts (power assertion,
  Bonjour, Keychain) sit behind the same portable boundaries as the rest of the daemon.
- **Several Macs from one phone** is not required; the design does not prevent it.

## Choices left to the implementing agent

The owner delegated these on 2026-09-26. Each can still be changed by the owner.

| Question | Choice |
| --- | --- |
| App stack | Expo for the simulator milestone. Measured on the owner's iPhone as the first device step; Flutter if it misses the speed budget (AC-115). |
| Where Rust is used on the phone | Only the encrypted session, if the binding is clean. Otherwise a vetted library with shared test vectors. |
| Sound with the door | None. |
| Which addresses may connect | Private and link-local ranges only. A setting can add the VPN range. |
| App lock | Off by default. A setting turns it on. |
| Text in notifications | Agent name and event kind only. A setting can add the agent's last line. |
| Stop the daemon from the phone | Mac only. *Stop all agents* is available on the phone. |
| Start an agent in a folder Overseer has never used | Not from the phone in this gate. |
| Does a connected phone count as a watching UI (AC-45)? | Yes while the app is in the foreground; the Mac's banner is replaced by the phone's notification when the phone opted in. |
| Distribution | A development build on the owner's iPhone, then TestFlight. App Store with the relay gate. |
| Android emulator | A new virtual device made for Overseer. The ones already on this Mac belong to other projects and are left alone. |

## Simulators first

The first milestone is built and verified on the iOS simulator and the Android emulator. It needs
no Apple account, no signing and no push key.

| Part | On the simulators now | On the iPhone later |
| --- | --- | --- |
| Pairing | By typing the code | By scanning the code |
| Finding the Mac | Through loopback and a manual address | Bonjour and the local network permission |
| Notifications | The daemon's exact payload, delivered by the simulator's own tool | Through Apple's push service, on a locked phone |
| Speed | Baselines, so a slow change is caught | The budget itself |
| The door | Frame recordings | Frame recordings at the phone's real refresh rate |
| Everything else | Fully verified | — |

A criterion whose Verify clause names the iPhone stays unchecked until that part is done. Its
record says what is proven on the simulators and what waits for the device. A simulator never
satisfies a device check.

## Steps for the owner

The implementing agent cannot do these: they need the owner's password, the owner's agreement or
the owner's Apple account.

### Now, so the iOS simulator can run

Checked on this Mac on 2026-09-26: Xcode 27.0 is installed, its license has not been accepted,
and the active developer directory is the command line tools. The Android tools and an Android 35
system image are installed and need nothing from the owner.

1. Accept the Xcode license:

   ```bash
   sudo xcodebuild -license
   ```

2. Point the developer tools at Xcode:

   ```bash
   sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
   ```

3. Open Xcode once and let it install the iOS simulator, if it offers to.

### Later, for the real iPhone

Nothing here reuses an identifier, key or profile from another project.

1. In the Apple Developer account, register a new app identifier for Overseer (proposed:
   `com.beelol.overseer.phone`) with the Push Notifications capability.
2. Create a new push key for Overseer. Choose a key for that one app identifier only, not a key
   for the whole team. Download it once and note its key ID and the team ID.
3. Give the key to Overseer on the Mac with the command the implementation provides. It is stored
   in the macOS Keychain and nowhere else.
4. Connect the iPhone, choose the team for signing in Xcode, and install the development build.
5. On the iPhone, allow Overseer to find devices on the local network and to send notifications.
6. Turn phone access on in VS Code, choose Pair a Phone, and scan the code. This is the only
   pairing.
7. Tell the implementing agent the phone is paired. It runs the device checks: the speed budget,
   the door, a push on the locked phone, and a changed address.
8. Do the phone session (AC-133) and mark the door and the transitions on the review page.

## Phases (goal candidates)

Each phase is independently useful and verifiable; the criteria are in the main RFC.

| Phase | Criteria | Outcome |
| --- | --- | --- |
| 1. Prove | AC-115 | On the simulators: the door and a long streaming conversation, the encryption on both sides, a handled notification, and the reuse decision. Nothing is locked in before this. |
| 2. Connect | AC-116, AC-117, AC-118, AC-119, AC-120, AC-134, AC-141 | The gateway with its desktop switch, pairing once, encryption, devices and discovery, with a minimal app built on the platform layer and the generated protocol types. |
| 3. Hold | AC-121, AC-122, AC-123 | Resume from the cursor, exactly-once requests, an awake Mac. |
| 4. See and control | AC-124, AC-125, AC-126, AC-127, AC-131 | Agents, conversations, control, review and the rest of Overseer, in one app for iOS and Android. |
| 5. Feel | AC-135, AC-136, AC-137 | The speed budget, the door, and motion throughout. |
| 6. Needs you, safely | AC-129, AC-130 | Notifications with their switches, and safety without friction. |
| 7. Overseer itself | AC-128 | The chat with Overseer on the phone. Waits for AC-107. |
| 8. Confirm on the simulators | AC-132 | Regression coverage. The simulator milestone is complete. |
| 9. The real iPhone | The device parts of AC-115, AC-117, AC-120, AC-129, AC-135 and AC-136, then AC-133 | After the owner's device steps: the speed budget and the stack decision first, then discovery, push, and the owner's session. |

## Acceptance

AC-115 to AC-137 and AC-141 in the main RFC are the acceptance criteria. Their Verify clauses cover, in short:

- spikes that prove the parts on the simulators, and the stack confirmed by measuring on the
  owner's iPhone;
- a switch on the desktop, and a phone that says off when it is off;
- one pairing that survives restarts, updates, a changed address and a month away;
- a gateway that is closed by default, answers nothing before authentication, and leaves the local
  socket boundary untouched;
- pairing that needs the Mac, with expiry, single use and lockout;
- a packet capture with no readable protocol, and refusals for unknown, tampered and replayed input;
- revocation within a second, and a scope check that covers every method by construction;
- reconnecting after an address change without pairing again, and refusing an impostor;
- event streams that are identical to the daemon's log after a hundred random disconnects;
- requests that act once no matter how often they are retried;
- a power assertion that is held during runs and released after;
- the phone's list and conversations equal to the daemon's state and VS Code's chat;
- the same launch records from the phone as from VS Code, one answer when two surfaces answer
  at once, and one tiny live turn per harness;
- diffs equal to Git's, hunk actions checked on disk, and file methods that cannot leave the workspace;
- no credential in any traffic, and every daemon method listed with its phone status;
- notifications that send nothing when switched off, and on a locked iPhone arrive within five
  seconds and are answered from the notification;
- a security review, a fuzz test, confirmations for everything destructive, and an app that
  opens twenty times without asking for anything;
- both platforms in both themes, following the system setting, matching VS Code's Overseer
  themes from one token source, accessible, each with its own conventions;
- no platform test outside the platform layer, and protocol types that cannot drift;
- every speed budget met on the owner's iPhone, and a slow change failing the run;
- the door matching the launch screen frame for frame in both modes, on a cold start only, never
  adding to the launch time;
- every transition recorded, on tokens, and marked right by the owner;
- the owner's dated confirmation after a real session on their iPhone.
