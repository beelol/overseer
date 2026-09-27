# Side RFC: phone remote on the same network

Status: proposed by the owner on 2026-09-26. Acceptance criteria: AC-115 to AC-137 (Gate N) in the
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
| App stack | The best option that is not slow. Expo is acceptable. Rust is not forced: it is used only where it is needed. |
| Platform behaviour | Separated per platform behind generic interfaces. Cross-platform libraries are used where they fit. |
| Speed | The app is hyper fast. |
| Motion | Cool animations, with a signature launch: a sci-fi gradient door carrying a grayscale logo that splits open diagonally when the app is ready. |

## Proposed defaults, distinguished from the decisions above

These are the implementing agent's proposals. The owner can change any of them; a change is a
recorded revision of this RFC.

- **Flutter for the app**, in `phone/`, because it is compiled ahead of time and draws every
  frame itself. Expo with React Native, which the owner accepted, is the second candidate. AC-115
  measures both on the owner's iPhone and the numbers decide (see [The app stack](#the-app-stack)).
- **Rust on the phone only for the encrypted session**, and only if the spike shows a clean
  binding. Everything else is written in the app's own language.
- **The door shows on a cold start only**, never when returning from the background.
- **Message-layer encryption** with an established handshake (Noise IK proposed), so the same
  session can later cross a relay that sees only ciphertext. Chosen in AC-115.
- **Full control is the default scope** of a paired phone; *Watch only* exists as a choice.
- **Three things stay on the Mac**: pairing a device, managing devices, and stopping the daemon.
  A phone that could do these could lock itself out or let another device in.
- **Review on the phone** reads diffs and accepts or rejects hunks. Editing files on the phone is
  not in this gate.
- **New agents start in repositories Overseer already knows.** Typing an arbitrary path on the
  phone is not in this gate.
- **Notifications carry no prompt, code or file content** by default.
- **Push on iOS only** in this gate, sent by the daemon straight to Apple with the owner's key.
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
- **Off by default.** *Overseer: Turn On Phone Access* (and `overseerd ctl gateway.enable`) starts
  it; turning it off closes the listener and every session. The daemon persists and enforces the
  setting, so it holds with VS Code closed.
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
turn) to paired iPhones that opted in. It talks to Apple's push service directly, with the owner's
push key kept in the macOS Keychain. There is no Overseer server.

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
- Destructive actions from the phone name what will be lost and ask for the device's unlock
  (Face ID, Touch ID or passcode). The app locks after five minutes in the background.
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

React Native is not a web page in an app: its screens are real native views, and it is fast enough
for many well-known apps. It is still JavaScript at run time, and the owner asked for hyper fast
with heavy custom animation, which is where a compiled stack with its own renderer has the
advantage. Two native apps would be fastest and would double every screen. The proposal is
Flutter, with the choice made by AC-115's measurements rather than by opinion.

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

- **Closed.** A dark, sci-fi gradient in the Overseer theme's colors fills the screen. A grayscale
  Overseer logo (from `extension/media/overseer.svg`) sits across a diagonal seam.
- **Waiting.** A slow light travels along the seam, so the door is clearly alive.
- **Opening.** When the first screen is drawn, the door splits along the seam. The halves slide
  apart and the logo splits with them. The app is already in place underneath.
- **No jump.** The system's launch screen shows the same closed door in the same place.
- **Never slower.** The door opens as soon as the first screen is ready from the cache. It does
  not wait for the connection and has no minimum time on screen. Launch to a usable list is the
  same with the door turned off.
- **Cold start only**, about 600 ms, interruptible by a touch. With Reduce Motion on it fades.

The rest of the app moves with the same care. One motion system, with durations, easing and
springs as tokens, drives every transition. Motion explains where a thing came from or what
changed. It can be interrupted, follows the finger, and is replaced by fades under Reduce Motion.
The owner marks the door and each transition on a review page until they look right.

## Testing

- **Daemon:** gateway protocol tests in `cargo test` with a real daemon binary, as today.
- **iOS simulator:** shares the Mac's network; connects through loopback. No camera, no push.
- **Android emulator:** connects to `10.0.2.2`. No Bonjour, no camera.
- **The owner's iPhone:** Bonjour, the local network permission, the camera, push notifications,
  Face ID, and performance. Only a real device can verify these.
- **Phone scenarios** run against a real daemon with fixture harnesses, from one command.
- **Speed** is measured on release builds, on the owner's iPhone and the emulator, by the same
  run that checks everything else.
- **Live turns** follow the owner's paid-turn rules: tiny prompts, one attempt per step.

## Limits and out of scope

- **Away from home.** Different networks need the relay (its own RFC and gate). A VPN that puts
  phone and Mac on one private network may carry the same connection; it is untested and not
  claimed by this gate.
- **The Mac must be on.** Nothing runs in the cloud. A sleeping or shut Mac is unreachable.
- **Editing files on the phone**, typing an arbitrary repository path, Android push, tablets,
  watch apps, widgets and App Store distribution are later work.
- **Linux hosts** belong to AC-41. The gateway's platform-specific parts (power assertion,
  Bonjour, Keychain) sit behind the same portable boundaries as the rest of the daemon.
- **Several Macs from one phone** is not required; the design does not prevent it.

## Open questions

| Question | Recommendation |
| --- | --- |
| App stack | Flutter, confirmed by AC-115's measurement against Expo on the owner's iPhone. The owner can name the stack instead and skip the comparison. |
| Where Rust is used on the phone | Only the encrypted session, if the binding is clean. Otherwise a vetted library with shared test vectors. |
| When the door shows | On a cold start only. |
| The door in the light theme | A light variant. The dark door is the default. |
| Sound with the door | None. |
| Default scope of a new device | Full control, as the owner asked. |
| Which addresses may connect | Private and link-local ranges only. A setting can add the VPN range. |
| App lock | On, after five minutes in the background. |
| Text in notifications | Agent name and event kind only. A setting can add the agent's last line. |
| Stop the daemon from the phone | Mac only. *Stop all agents* is available on the phone. |
| Start an agent in a folder Overseer has never used | Not from the phone in this gate. |
| Does a connected phone count as a watching UI (AC-45)? | Yes while the app is in the foreground; the Mac's banner is replaced by the phone's notification when the phone opted in. |
| Distribution | TestFlight to the owner's devices. App Store with the relay gate. |

## Phases (goal candidates)

Each phase is independently useful and verifiable; the criteria are in the main RFC.

| Phase | Criteria | Outcome |
| --- | --- | --- |
| 1. Prove | AC-115 | The app stack chosen by measurement, the encryption on both sides, Bonjour and push on a real iPhone, and the reuse decision. Nothing is locked in before this. |
| 2. Connect | AC-116, AC-117, AC-118, AC-119, AC-120, AC-134 | The gateway, pairing, encryption, devices and discovery, with a minimal app built on the platform layer and the generated protocol types. |
| 3. Hold | AC-121, AC-122, AC-123 | Resume from the cursor, exactly-once requests, an awake Mac. |
| 4. See and control | AC-124, AC-125, AC-126, AC-127, AC-131 | Agents, conversations, control, review and the rest of Overseer, in one app for iOS and Android. |
| 5. Feel | AC-135, AC-136, AC-137 | The speed budget, the door, and motion throughout. |
| 6. Needs you, safely | AC-129, AC-130 | Push notifications and the safety rules. |
| 7. Overseer itself | AC-128 | The chat with Overseer on the phone. Waits for AC-107. |
| 8. Confirm | AC-132, AC-133 | Regression coverage; the owner's session on their iPhone. |

## Acceptance

AC-115 to AC-137 in the main RFC are the acceptance criteria. Their Verify clauses cover, in short:

- spikes that prove the parts, and an app stack chosen by measuring both candidates on the
  owner's iPhone;
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
- the same launch records from the phone as from VS Code, and one tiny live turn per harness;
- diffs equal to Git's, hunk actions checked on disk, and file methods that cannot leave the workspace;
- no credential in any traffic, and every daemon method listed with its phone status;
- a notification on a locked iPhone within five seconds, answered from the notification;
- a security review, a fuzz test, and confirmations for everything destructive;
- both platforms in both themes, accessible, each with its own conventions;
- no platform test outside the platform layer, and protocol types that cannot drift;
- every speed budget met on the owner's iPhone, and a slow change failing the run;
- the door matching the launch screen frame for frame, never adding to the launch time;
- every transition recorded, on tokens, and marked right by the owner;
- the owner's dated confirmation after a real session on their iPhone.
