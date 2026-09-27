# Side RFC: Voice Mode — talk to Overseer, redirect every agent

Status: owner request (2026-09-27). Proposed; nothing is built. Tracked by AC-162 to AC-177 under
[Gate R](../overseer-rfc.md#gate-r--voice-mode-added-by-the-owner-2026-09-27) in the main RFC.
The defaults below stand until the owner changes them; the
[questions for the owner](#open-questions-for-the-owner) are at the end.

## Why

With several agents running, the owner's eyes and hands are busy: reading a diff, watching the
grid, or away from the keyboard. Redirecting three agents today means opening three chats and
typing three messages. Speaking is faster. One sentence, and Overseer works out which agents it is
for, tells each of them, and shows exactly what it told them.

A voice that cuts in, stops at every cough, acts on half a sentence or says "done" when nothing
was sent is worse than typing. So most of this document is about three things: holding the floor
well, answering fast, and proving what was sent.

## The owner's request

In the owner's words (2026-09-27):

> a voice mode that you can constantly talk to that changes and redirects course across all
> orchestrated agents depending on the context you're talking. it should not interrupt super
> easily but should always flow and listen to what you're saying and quickly respond that it's
> working on the request and show evidence of what it's saying to all new subagents and all in
> flight ones that it references for that request.

Added the same day, about what is on screen:

> it should also have the animated logo in the middle while that convo is happening. some effect
> on the logo from the wave form so you know its getting what you're saying. not sure the best
> way to animate it

## Decisions from the owner

| Topic | Requirement |
| --- | --- |
| Always there | The owner can talk to it constantly. While it is on it listens; no button per sentence. |
| Reach | It changes and redirects course across all orchestrated agents. |
| By context | Which agents a sentence is for comes from the context of what is being said. |
| Hard to interrupt | It does not interrupt easily. |
| Flow | It always flows and keeps listening to what is being said. |
| Quick answer | It quickly answers that it is working on the request. |
| Evidence | It shows what it is saying to every new agent and every in-flight agent it references for that request. |
| Audio | Audio is collected on the Rust side (owner, 2026-09-27, added while this RFC was written). No Swift or webview code captures the microphone. |
| The mark | The animated Overseer logo is in the middle while the conversation is happening. The waveform has an effect on it, so the owner knows Overseer is getting what is said. How to animate it is left open. |

## Proposed defaults, distinguished from the decisions above

These are this RFC's choices, not the owner's. A change is a recorded revision.

- **Off until asked.** Like Audio Mode, Voice Mode is off on a new install.
- **A Rust listener, in its own process.** The code that collects audio is a crate in the Cargo
  workspace. The daemon starts it as a separate process, so a fault in audio code or in a speech
  model cannot take the daemon and its agents down. It is one binary with no UI.
- **On the Mac.** The owner decided on 2026-09-26 that the phone has no voice mode (Gate N). That
  stands. The phone neither listens nor speaks.
- **Speech stays on the Mac.** Turning speech into words, and words into speech, happens on the
  machine. Audio is never stored and never sent anywhere.
- **The daemon owns it.** `overseerd` owns the voice session, as it owns Audio Mode. It works with
  VS Code closed, and two open windows never handle one sentence twice.
- **One Overseer.** Voice Mode is the spoken side of Talk to Overseer (AC-107). Typed and spoken
  messages are one conversation with one memory.
- **Open floor.** While Voice Mode is on, everything the owner says is heard without a wake word.
  *Name first* and *push to talk* are settings for shared rooms.
- **Steering needs no separate yes.** A spoken redirect is sent after a short settle window in
  which the owner can correct or cancel it. Actions that cannot be taken back still wait for a yes.
  This differs from the typed chat of AC-107, where every action waits for a yes.
- **The model proposes, the daemon sends.** The orchestrator's model writes a plan. The daemon
  checks it, sends the messages and records them. What the card shows is what was sent.
- **Add by default, stop when the course changes.** A message waits for the end of the agent's
  running turn unless the request changes what the agent is doing now.
- **Short speech.** One sentence per answer. No paths, identifiers or code read aloud.
- **English first.** Other languages follow what the chosen recognizer supports.

## Vocabulary

| Word | Meaning |
| --- | --- |
| Utterance | One stretch of the owner's speech, from its first word to the end of the thought. |
| Request | An utterance that asks for something. It gets an id (`V-0042`) and a card. |
| Target | An agent a request is for: *in flight* (an existing run) or *new* (started for the request). |
| Dispatch | One message from Overseer to one target, as part of a request. |
| Settle window | The time between Overseer's spoken answer and the send, in which the owner can correct or cancel. |
| Floor | Who is speaking now: the owner, Overseer, or nobody. |
| Backchannel | The sounds of listening: "mm-hm", "yeah", "okay", "right". Not a request. |
| Orchestrator | The one Overseer session (AC-107) that reads the agents' state and writes plans. |
| Listener | The Rust process, started by the daemon, that collects audio from the microphone, turns it into words and plays Overseer's speech. |

## Shape

```
  microphone                    Mac (all Rust)
      │
      ▼
  ┌────────────────────────┐  words, levels    ┌────────────────────────────────────┐
  │ listener (Rust)        │ ────────────────► │ overseerd                          │
  │  audio capture         │                   │  ├─ voice session                  │
  │  echo cancellation     │ ◄──────────────── │  │   utterances, requests,         │
  │  speech detection      │  lines to speak   │  │   targets, dispatches           │
  │  speech to words       │                   │  ├─ audio arbiter (cues + speech)  │
  │  speaking              │                   │  ├─ orchestrator session (warm)    │
  └────────────────────────┘                   │  └─ runs ── run.follow_up,         │
      │                                        │            run.interrupt,          │
      ▼                                        │            task.create             │
   speakers                                    └────────────────────────────────────┘
                                                      ▲
                          VS Code, terminal UI, phone: show the voice strip and the cards
```

- The listener collects audio in Rust, from the system's audio input. Candidates for the spike
  (AC-162): `cpal` for capture; the system's voice-processing unit or WebRTC's audio processing
  for echo cancellation; whisper.cpp through Rust bindings, or Apple's on-device recognizer where
  it can be reached from Rust, for words.
- The listener plays Overseer's speech itself. It then knows exactly what the speakers are
  playing, which echo cancellation needs, and it can lower or stop the voice within the budget.
- The listener sends **words** and **loudness levels**, never audio, over the daemon's owner-only
  socket (AC-08).
- The orchestrator never talks to an agent. It writes a plan; the daemon sends through the same
  methods the chat uses, so every message is an ordinary event in the agent's run.
- UI clients draw what the daemon reports. None of them listens or speaks.

## A request, start to finish

```
14:32:05.0  owner   "Phone and Continuity should both use the new wire format, and
                     someone should write the migration note."
14:32:09.1          end of the thought (0.7 s of silence after a complete sentence)
14:32:09.3  heard   a soft signal; the words are in the voice strip
14:32:10.4  answer  "On it: telling Phone and Continuity, and starting one agent for the note."
14:32:10.4  card    V-0042 appears with three targets, held for 2 s
14:32:12.4  send    Phone: redirect · Continuity: add · Migration note: new agent
14:32:13.0  done    "Sent. Phone stopped and picked it up. Continuity gets it after this turn."
```

## Listening and the floor

"Does not interrupt easily" can be read three ways. All three are covered, because each one
breaks the flow when it goes wrong.

| Who interrupts whom | Rule |
| --- | --- |
| Noise, or the owner, interrupts Overseer's speech | Overseer keeps talking unless the owner really speaks to it. |
| Overseer interrupts the owner | Never. It waits for the end of the thought and for a free floor. |
| Overseer interrupts an agent | Only when the request changes what the agent is doing now. See [Sending](#sending-add-redirect-stop). |

### The end of a thought

A pause is not the end. People stop to think in the middle of a sentence.

| Situation | The utterance ends after |
| --- | --- |
| The words read complete | 0.7 s of silence |
| The words read unfinished (they end in "and", "so", "the", "to", "um", a comma) | up to 2.5 s of silence |
| 2.5 s of silence | always |
| 90 s of speech | Overseer takes what it has as one utterance and keeps listening |

Complete or unfinished is decided on the Mac from the last words and the recognizer's
punctuation. No model turn is spent on it. The spike (AC-162) tests the rule on real pauses.

### What never counts

None of these starts a request, and none of them stops Overseer's speech:

- noise, typing, music, a cough, a door;
- sound that does not become words;
- backchannels;
- Overseer's own voice and Audio Mode's cues coming back through the microphone (the listener
  cancels the Mac's own output).

### When the owner speaks over Overseer

| The owner | Overseer |
| --- | --- |
| says one word or a backchannel | keeps talking |
| says two or more words | lowers its voice within 150 ms and keeps listening |
| is still speaking 0.7 s later | stops at the end of its phrase |
| says "stop", "wait" or "hold on" | stops within 300 ms |

Lowering first and stopping second is what makes it hard to knock over and still quick to yield.
What Overseer had not said yet stays in the card. "Go on" says the rest.

### When Overseer wants to speak

- It never starts speaking while the owner is speaking.
- A line waits for a free floor. After 20 s it goes to the card alone.
- Listening never stops: not while Overseer thinks, and not while it speaks.
- More words from the owner inside the settle window join the same request. Later words start a
  new request that knows about the previous one.

### Who is being addressed

| Setting | Behaviour |
| --- | --- |
| Open floor (default) | Every utterance is heard. The orchestrator drops speech that is not for it. |
| Name first | Saying "Overseer" or an agent's name opens the conversation. It stays open during the exchange and for 60 s after. |
| Push to talk | Overseer listens while a key is held. |

Open floor is what the owner asked for. Its cost is stated under
[Privacy and security](#privacy-and-security): the words of everything the owner says in the room
reach the orchestrator's model.

### Mute and calls

- One global shortcut and one command mute Voice Mode. Muted means the microphone is closed and
  macOS's own indicator goes off. It does not mean "listening but ignoring".
- When another app starts using the microphone (a call, a recording), Voice Mode pauses by itself,
  says so in the strip, and resumes when the other app is done.

## The answer: heard, working on it, done

| Answer | What | Budget, from the end of the thought |
| --- | --- | --- |
| Heard | A soft signal; the words appear in the voice strip | 300 ms (p95) |
| Working on it | One spoken sentence: what will be done and for whom | starts within 1.5 s (p50), 2.5 s (p95) |
| Holding line | "Working on it.", once, if the sentence above is late | at 2.5 s |
| Done | One short line once the messages are out | when the last dispatch is sent |

- **Heard** is decided on the Mac. It needs no model.
- **Working on it** is the first thing the orchestrator writes, before it reads anything. It is in
  the future tense: "telling", "starting".
- **Done** is written by the daemon from the dispatches' recorded states, not by the model. So
  Overseer cannot say "sent" about a message that was not sent.
- The orchestrator session is kept warm (one long-lived session per daemon), because starting a
  harness per sentence would miss the budget.

The budgets are targets. The spike measures them on the owner's Mac; a budget that cannot be met
is changed by a recorded decision, not quietly.

## Working out who it is for

The daemon gives the orchestrator a small, bounded view for every request:

| Part | Content |
| --- | --- |
| The conversation | The utterance, and the last 10 minutes or 30 exchanges. |
| Focus | The agent the owner has selected or is tracking, and what it last said. |
| Roster | Every active top-level agent: name, repository, branch, status, first line of its task, a one-line summary of its last message, files it changed, number of children. |
| Recent events | Agents that asked, finished or failed in the last 10 minutes. |
| Previous requests | The last 10 requests and their targets. |

The daemon computes **candidates** with a reason. The orchestrator chooses among them and writes
each message. A target that is not an active run is refused.

| Reason | Example |
| --- | --- |
| Named | "Tell **Continuity** to…", "the **phone** one", "whoever has **AC-116**" |
| Focus | "This one should also…" while an agent is selected |
| Just asked | "Yes, do that" after an agent asked a question |
| Previous targets | "Tell them also to update the ledger" |
| Owns the subject | "The wire format changed" reaches the agents that touched that file |
| Everyone | "Everybody stop pushing to main" |

| Confidence | What happens |
| --- | --- |
| High | Sent after the settle window (2 s). |
| Medium | Sent after a longer window (4 s); the spoken answer names every target. |
| Low | Nothing is sent. Overseer asks one short question: "Phone or Continuity?" |

Native children are reached through their top-level agent. Overseer cannot message a harness's
own child directly; the card says *through Phone*.

Text from agents, files and the screen is information for the orchestrator. It is never treated
as the owner's words, and it cannot add a target.

## Sending: add, redirect, stop

| Delivery | When | What the agent sees |
| --- | --- | --- |
| Add (default) | The request adds to the work or does not conflict with what the agent is doing | The message at the end of its running turn |
| Redirect | The request changes what the agent is doing now, or the owner says "now", "instead", "drop that" | Its turn is stopped and the next one starts with the message |
| Stop | "Stop Phone" | Its turn is stopped. No message, no settle window |

- An idle or waiting agent gets its message at once.
- Several requests for the same agent before its turn ends become one message, in the order
  spoken. The agent is not stopped more than once for them.
- The card records the delivery and its reason ("it is writing the old format now").
- `voice.delivery` can force *always add* or *always redirect*.

## New agents

A request can start agents ("someone should write the migration note").

- Each gets a prompt written for it, and the repository from the context.
- Harness, account, model and workspace mode are the composer's remembered defaults (AC-59).
- Up to three new agents per request without a yes; up to eight with one.
- A problem (unknown repository, signed-out account, harness not installed, untrusted workspace)
  is one spoken line and a row in the card with its fix. The other targets are still sent.
- When Swarm and Auto are merged, new agents are started through them, inside their limits.

## Evidence

### The request card

One card per request, in the conversation with Overseer.

```
V-0042 · 14:32 · voice
"Phone and Continuity should both use the new wire format, and someone
 should write the migration note."

 Phone app        named       redirect   picked up 14:32:13   Switch the gateway client to…
 Continuity       named       add        queued               When this turn ends: use the…
 Migration note   new agent   start      running              Write the migration note for…
```

| Column | Content |
| --- | --- |
| Agent | Logo and name; a click opens its chat at the message. |
| Why | The reason it was chosen. |
| Delivery | Add, redirect, stop or start, with the reason on hover. |
| State | Held → sent → delivered → picked up → answered; or failed, cancelled, not sent. Each with its time. |
| Message | The first line; it opens to the whole text. For a new agent: the full prompt and its settings. |

The text in the card is the text that was sent, byte for byte. A state changes only when the
daemon records the event behind it.

### In each agent's chat

```
From Overseer (voice) · request V-0042 · 14:32
The owner said: "Phone and Continuity should both use the new wire format,
and someone should write the migration note."
For you: switch the gateway client to the wire format in
docs/rfcs/phone-remote-protocol.md. Stop writing the old format.
Also told: Continuity (same change), Migration note (new agent, writes the note).
```

- The owner's words are quoted whole, so the agent sees the source and not only a paraphrase.
- Every target knows who else was told.
- The daemon writes the header, the quote and the last line. The model writes only *For you*.

### Elsewhere

- Targeted agents carry a voice mark in the side bar and the grid while the request is open.
- Cards survive restarts and are found by the search.

## Correcting and cancelling

| When | The owner says | Result |
| --- | --- | --- |
| Inside the settle window | "no", "cancel", "wait" | Nothing is sent. The card reads *cancelled*. |
| Inside the settle window | "I meant the phone agent", "not Continuity" | The targets change. Only the new ones get a message. |
| Inside the settle window | more words | They join the same request. |
| After the send | a correction | A follow-up goes to the same agents and names the request it replaces. The first card reads *superseded*. |

A sent message cannot be unsent, and nothing pretends otherwise.

## What voice may do

The daemon enforces the tiers, whatever the plan says.

| Tier | Actions | How |
| --- | --- | --- |
| Look | Questions about the agents; select or track an agent; open the grid; mute | At once |
| Steer | Add, redirect, stop an agent or every agent; up to three new agents | Answer, settle window, send. Stop skips the window |
| Confirm | Answer a permission request; merge back; open a pull request; archive; more than three new agents | Read back in one sentence, then a yes by voice or click within 20 s. Silence or anything unclear is a no |
| Not by voice | Accounts and sign-in; phone access and pairing; Continuity's download and install settings; workspace cleanup; stopping the daemon; changing these rules | Overseer opens the place in the UI and says so |

Permission requests are answered one at a time. "Allow everything" is refused.

## Audio Mode and Voice Mode together

One audio arbiter in the daemon decides what plays.

| Situation | Rule |
| --- | --- |
| A cue is due while Overseer speaks | It waits for the end of the phrase. |
| A routine cue is due while the owner speaks | It is dropped. |
| An attention cue is due while the owner speaks | It waits for the end of the thought, at most 5 s. |
| The heard signal | It reuses one of the twelve Reactor keys. No sound is added or generated. |

Each mode works with the other off. Audio Mode's rules and tests do not change.

## When things fail

| Failure | Behaviour |
| --- | --- |
| The orchestrator cannot be reached (offline, signed out, rate-limited) | Overseer says so once. The request stays in its card as *not sent*. Nothing is sent later by itself. |
| Continuity is on (Gate L) | The orchestrator follows its failover: another provider, then a local model inside the memory budget (AC-140). |
| No model at all | Built-in phrases still work: stop an agent by name, stop everyone, mute, what is running. |
| The recognizer or the listener fails | Logged and shown in the strip. The daemon restarts the listener at most three times in ten minutes. |
| Anything in Voice Mode | It never stops, delays or fails an agent. |

## Privacy and security

- **On the Mac.** Recognition and speech are on-device. The listener opens no network connection.
- **No recordings.** Audio lives in the listener's memory for at most 30 s and is never written to
  disk.
- **Levels.** The loudness values that move the mark are numbers, not sound. They are sent at
  most 30 times a second, cannot be turned back into speech, and are never stored.
- **What is stored.** The words of requests, redacted like other events, for 30 days or 5,000
  requests. Speech that was not a request lives only in the rolling context (10 minutes, memory).
- **What reaches a model.** The words of each utterance that passes the gate go to the
  orchestrator's model on the owner's own account, as typed text in Talk to Overseer does. With
  the open floor that includes talk that was not meant for Overseer. *Name first* limits it to
  what is addressed.
- **Other voices.** The Mac's own output is cancelled, so a video cannot give orders. Another
  person in the room can. Telling voices apart is not in this gate; the tiers are the protection.
- **The microphone permission.** macOS asks once, and must ask in Overseer's name with the
  Overseer mark (AC-142), not in the name of VS Code or a terminal. A bare binary gets no name of
  its own, so the spike finds out what the Rust listener needs: a usage description embedded in
  the binary, or a minimal bundle around it that holds only the Rust binary, its identifier and
  the icon. No other project's Apple assets are used.
- **Paid turns.** Each request is an orchestrator turn on the owner's account. The local gate
  (speech, words, not a backchannel) and a cap on requests per hour keep an open microphone from
  spending by itself. The strip shows the day's requests and usage.

## Settings

Stored and enforced by the daemon.

| Setting | Default | Range |
| --- | --- | --- |
| `voice.enabled` | off | |
| `voice.floor` | open | open, name first, push to talk |
| `voice.delivery` | auto | auto, always add, always redirect |
| `voice.settleSeconds` | 2 | 0 to 10 |
| `voice.speak` | all | all, first answer only, none (cards only) |
| `voice.voice`, `voice.rate` | the system voice | installed voices |
| `voice.input`, `voice.output` | system default | the Mac's devices |
| `voice.permissionAnswers` | on | on, off |
| `voice.newAgentsPerRequest` | 3 | 0 to 8 |
| `voice.requestsPerHour` | 120 | 10 to 600 |
| `voice.keepDays` | 30 | 1 to 365 |

## Protocol and data

Proposed names. The implementation may change them with the reason recorded.

| Method | Purpose |
| --- | --- |
| `voice.get` | State, settings, availability, devices, installed voices |
| `voice.set` | Change a setting; refuses values out of range and turning on where unavailable |
| `voice.mute` | Mute or unmute |
| `voice.say` | An utterance as words. The listener uses it; tests use it with fixture words |
| `voice.focus` | A UI client reports the selected or tracked agent |
| `voice.requests` | List and search requests |
| `voice.cancel`, `voice.confirm` | Cancel an open request; answer a read-back |

| Event | Content |
| --- | --- |
| `voice.state` | off, listening, hearing, thinking, speaking, muted, paused |
| `voice.heard` | Partial and final words of an utterance |
| `voice.request` | A request created or changed |
| `voice.dispatch` | One dispatch's state change, with the run's event id |
| `voice.spoke` | A line spoken, lowered, stopped or sent to the card alone |
| `voice.level` | The loudness of the owner's voice or of Overseer's, at most 30 a second; not stored |

| Record | Fields |
| --- | --- |
| `VoiceRequest` | id, heard at, words, kind, tier, state, settle until, spoken answers, superseded by |
| `VoiceDispatch` | request, run or new task, reason, confidence, delivery and its reason, message, hash, state with times, event ids |

## The mark in the middle

While a voice conversation is happening, the Overseer mark (AC-142) is in the middle and moves
with the voice. It answers one question at a glance: is Overseer getting what I am saying?

| State | What the mark does |
| --- | --- |
| Listening | Calm, slow motion. It is on and nobody is speaking. |
| Hearing you | It moves with the owner's voice, within 100 ms of the sound. |
| Thinking | Its own steady motion, not tied to any sound. |
| Speaking | It moves with Overseer's own voice. |
| Muted, paused for a call | Still and dimmed, with the mute or pause sign. |

- **A still mark means it does not hear.** Noise that does not count as speech leaves the mark
  calm. So when the owner speaks and the mark does not move, Overseer is not getting it.
- **Where.** In the centre of the voice view, which takes the middle of the editor area like the
  home chat (AC-72). Beside a review or the grid the same mark is shown small in the voice strip,
  so the work is not covered.
- **Levels, not audio.** The listener sends one loudness value at most 30 times a second. Speech
  cannot be rebuilt from it, and it is never stored. Every window draws from the same levels.
- **States without colour alone.** Each state differs in motion and shape, so it reads in
  grayscale and for colour-blind eyes.
- **Reduced motion.** With reduced motion on, the mark is still and a small level meter shows the
  voice.
- **Cheap.** 60 frames a second without slowing the views beside it. Nothing is drawn while the
  view is hidden.

The owner is not sure of the best way to animate it. So at least three candidates are built and
the owner picks on a review page.

| Candidate | The effect |
| --- | --- |
| Ring | The mark stays as drawn. A ring around it carries the waveform of the voice. |
| Orbit | The three swooshes turn around the core, wider and faster with the voice. |
| Star | The star at the core brightens and the glow behind the mark breathes with the voice. |

*Orbit* needs the mark in layers (swooshes, core, star). If the owner's files are flat images,
the effects are drawn around and behind the mark and the mark itself is not cut up.

The [brand rules](../design/brand.md) say the mark gets no effects. This is the one exception,
at the owner's request: only here, only the chosen animation, and never stretched or recoloured.

## UI

- **Mark.** The animated mark, as above.
- **Voice strip.** One line with Talk to Overseer, and a status bar item: the state, the words as
  they are heard, mute.
- **Cards.** In the conversation with Overseer, as above.
- **Marks.** A voice mark on targeted agents in the side bar and the grid.
- **Keyboard.** Turn on and off, mute, cancel the open request, yes and no to a read-back.
- **Text.** Every spoken line is also text. Design tokens, the three Overseer themes and the text
  budget apply.
- **Terminal UI and phone.** They show state and cards under their own criteria when taken up.

## Bounds

| Resource | Bound |
| --- | --- |
| Audio in memory | 30 s, in the listener; never on disk |
| One utterance | 90 s |
| Rolling context | 10 minutes or 30 exchanges |
| Open requests | 4; a fifth waits |
| New agents per request | 3 without a yes, 8 with one |
| Requests per hour | 120; then Overseer says so and takes built-in phrases only |
| One message | 4,000 characters |
| Speech queue | 3 lines; a line older than 20 s goes to the card alone |
| Request records | 5,000 or 30 days |
| Listener | one per daemon; at most 3 restarts in 10 minutes |
| Speech model | inside Gate L's memory budget, checked before it is loaded |
| Loudness levels | 30 a second at most; never stored |
| While off | no listener process, microphone closed, no recognizer loaded |

## Working alongside the other gates

| Gate | Relation |
| --- | --- |
| Gate M, Talk to Overseer (AC-107) | The orchestrator session moves from the extension into the daemon so voice works with VS Code closed. The typed chat becomes a client of it and keeps its rule (propose, then yes). This touches Gate M's area and is said so in the pull request. |
| Gate O, Audio Mode | One arbiter; Audio Mode is unchanged. |
| Gate L, Continuity | The orchestrator follows its failover and its memory budget. A speech model counts against the same budget. |
| Gate N, phone | No voice on the phone. Cards appear there when the chat with Overseer does (AC-128). |
| Swarm and Auto | New agents go through them once they are merged. |
| Main RFC | "No embedded inference model" is revised for speech only: a recognizer turns speech into words on the Mac and decides nothing about agents. |

## Testing

- **Words layer.** Most tests send fixture words with timings through `voice.say` and use a
  fixture orchestrator that returns scripted plans. They are deterministic and cost nothing.
- **Audio layer.** The listener, in test mode, reads audio from a file instead of the microphone.
  Test speech is synthesized at test time into a temporary folder and deleted. The repository
  holds no recorded or generated voice, as Audio Mode requires.
- **Live.** A small sample on the default account inside the paid-turn budget, one attempt per
  step, recorded with its misses.
- **By voice.** The owner's session (AC-176).

## Limits and out of scope

- Telling the owner's voice from another person's.
- Voice on the phone; voice from outside the room.
- Online or streaming voices; cloned voices.
- Dictating code, or reading an agent's output aloud in full.
- Messaging a harness's native child directly.
- Linux and Windows: they report Voice Mode as unavailable.

## Open questions for the owner

| Question | This RFC's default |
| --- | --- |
| 1. "Not interrupt easily": noise and small sounds interrupting Overseer, Overseer cutting in on you, or Overseer stopping agents too quickly? | All three are covered. Say which matters most and the thresholds follow it. |
| 2. Open floor, or say "Overseer" first? | Open floor. Everything said in the room then reaches the model as words. |
| 3. May a spoken redirect go out without a yes? | Yes, after a 2 s settle window. Actions that cannot be taken back wait for a yes. |
| 4. May permission requests be answered by voice? | Yes, one at a time, after a read-back. |
| 5. Which voice? | The system voice at first; the owner picks by ear, as with the cues. |
| 6. Should Overseer speak up by itself when an agent needs you? | No. Audio Mode's cue does that. Overseer speaks only in answer. |
| 7. Should the typed chat follow the same tiers? | No change to AC-107 in this gate. |
| 8. Audio is collected on the Rust side: inside `overseerd` itself, or in a Rust process of its own? | Its own process, started by the daemon, so a fault in audio code cannot stop the agents. |
| 9. Which animation for the mark, and should the large mark also appear over a review or the grid? | The owner picks from three candidates. The large mark is in the voice view only; beside other work it is small, in the strip. |

## Order of work

One goal, written once the owner has answered the questions above. Built in its own worktree and
pull request.

| Step | Criteria | Outcome |
| --- | --- | --- |
| 0. Find out | AC-162 | The spike: audio capture in Rust, the recognizer, echo cancellation, the microphone permission, the orchestrator's speed. Decisions written here. |
| 1. Hear | AC-163, AC-164, AC-172, AC-173 | The Rust listener, the voice session in the daemon, the floor rules, the arbiter, privacy and bounds. Nothing is sent to agents yet. |
| 2. Answer and send | AC-165, AC-166, AC-167, AC-168, AC-171 | The three answers, targets from context, delivery, new agents, the tiers. |
| 3. Show and prove | AC-169, AC-170, AC-174, AC-175, AC-177 | Cards and messages, correcting and cancelling, the UI, the animated mark with its candidates, failures. |
| 4. Confirm | AC-176, the pick of AC-177 | The owner's session by voice, and the owner's choice of animation. |

## Acceptance

AC-162 to AC-177 in the main RFC are the acceptance criteria. Each has its Verify clause there.
