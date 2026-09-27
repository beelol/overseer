# Side RFC: Audio Mode — opt-in cues from the daemon

Status: owner request (2026-09-26). Tracked by AC-143 to AC-145 under
[Gate O](../overseer-rfc.md#gate-o--audio-mode-added-by-the-owner-2026-09-26) in the main RFC and
by T-23 and T-24 in the [TUI RFC](tui.md). The daemon and VS Code are built in pull request #5,
which carries the evidence; the terminal UI follows in pull request #6. What remains is in the
[prepared goal](audio-mode-goal.md).

## Why

With several agents running, the owner looks away. A short sound says an agent started,
finished or needs an answer without a glance at the screen. Sound that repeats, plays for
every tool call or plays twice because two windows are open is worse than none, so the rules
below are about staying quiet as much as about playing.

## Product decisions (made, not open)

- **Off until asked.** Audio Mode is off on a new install. It turns on only through an
  explicit action (*Overseer: Audio Mode and Reactor Cues…* → *Turn Audio Mode on*).
- **The daemon owns it.** `overseerd` stores the setting and does the classification, priority,
  deduplication, queueing and playback. Cues play with VS Code closed. Any number of UI
  clients can be attached; none of them plays, so none can repeat a cue.
- **Three events make a sound.** A top-level agent starts its first turn, completes, or
  needs the user. Everything else is silent by default.
- **One cue per need.** A permission request and the waiting status it causes are one need.
  Needs from several agents at the same moment play one cue; the UI shows how many wait.
- **Failing quietly.** A missing file, a missing player or a failed playback is written to
  the daemon log. It never stops, delays or fails an agent.
- **macOS in this phase.** Other platforms report playback as unavailable and refuse to turn
  Audio Mode on; nothing else changes for them.

## What makes a sound

Only runs without a parent are considered. The daemon listens to its own live event stream;
after a restart it does not replay cues for events that happened before.

| Event on a top-level run | Cue |
| --- | --- |
| `turn_started` for turn 1 | `agent_started` |
| status `completed` | `agent_complete` |
| status `waiting_for_user`, `failed` or `disconnected` | `agent_needs_attention`, once until the run leaves that state |
| later turns, `running`, `starting`, `queued`, `interrupted` | silent |
| permission events, tool calls, output, progress, reconnects | silent |
| anything on a child or grandchild run | silent |

The same cue within 800 ms is dropped, also when other agents' starts or completions arrive
in between. That is how simultaneous needs become one sound.

## Tracks

One track is selected for the whole daemon.

- **Reactor** (default). Twelve original synthesized MP3s bundled in the daemon binary:
  31,488 bytes together, each shorter than 0.5 s. They are the owner-approved selection;
  [`daemon/assets/reactor`](../../daemon/assets/reactor/README.md) lists each file's meaning,
  duration, size and SHA-256. Three keys play by themselves (`agent_started`,
  `agent_complete`, `agent_needs_attention`). The other nine (`agent_queued`,
  `agent_resumed`, `agent_progress`, `agent_stopped`, `agent_failed`, `agent_unblocked`,
  `review_ready`, `delivery_ready`, `verification_passed`) can be previewed and are reserved:
  a more specific event reuses one of the twelve keys instead of adding a sound.
- **System voice.** macOS `say` speaks "Agent started.", "Agent complete." or "An agent needs
  your attention." with the system default or an installed voice the user picks. Speech is
  produced on the Mac; nothing is downloaded and no speech file is stored.
- **Commander.** Optional. The user points Overseer at a private folder holding
  `<key>/transmission/commander.wav` for the three core keys. The daemon checks that they are
  WAV files, stores only the folder's path and plays the files where they are. They are
  never copied, cached, uploaded or committed. If the folder goes away the track reports
  unavailable.

## Ownership and protocol

Settings live in the daemon's database (`meta` keys `audio.reactor.enabled`, `audio.track`,
`audio.system_voice`, `audio.commander_dir`). Clients use five methods:

| Method | Purpose |
| --- | --- |
| `audio.get` | enabled, available, track, voice, whether a Commander folder is imported, the keys and the Reactor manifest |
| `audio.set` | change `enabled`, `track` or `voice`; refuses an unknown track, a voice that is not installed, Commander without a folder, and turning on when playback is unavailable |
| `audio.preview` | play one cue now, on request; works while Audio Mode is off |
| `audio.import_commander` | validate and remember a private folder |
| `audio.voices` | the installed system voices |

VS Code offers these through one command in the agents view's overflow menu and the command
palette. It never plays sound itself.

## Bounds

| Resource | Bound |
| --- | --- |
| Routine queue (start, complete, previews) | 4 cues; more are dropped |
| Urgent queue (needs attention) | 2 cues, taken before routine ones |
| Player processes | one at a time (`afplay` or `say`), gone when the cue ends |
| Attention history | 1,024 run ids |
| Reactor cache | at most the 12 bundled files, owner-only, under the daemon's data folder; written on first play and rewritten when the content differs from the bundled cue |
| While off | no player process, no cache folder |

## Owner decisions

- **A lost session sounds (2026-09-26).** The owner's cue ledger lists *disconnected* under
  **Agent stopped**, silent by default. The owner decided otherwise for a top-level agent whose
  session is lost while the daemon runs: it cannot continue without the user, so it keeps the
  attention cue. A session found lost when the daemon starts stays silent, because cues are
  never played for what happened while the daemon was down. AC-144 holds the rule and its test.
- **By ear (2026-09-26).** The checks prove which files are played, not how they sound. The
  owner confirms the result by ear (AC-145).

## Out of scope

Streaming or online voices, per-event customization and spoken detail for every event.
Controls in the terminal UI are not part of pull request #5; they are T-23 and T-24, built in
pull request #6.

## Acceptance

AC-143 is the criterion for the daemon and VS Code; the audio ledger
(`docs/verification/audio-mode.md`, in pull request #5) maps each part of it to a test or a live
check. AC-144 and AC-145 are the two loose ends of that pull request; T-23 and T-24 are the
terminal UI.
