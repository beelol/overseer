# Side RFC: Voice Mode — talk to Overseer, redirect every agent

Status: owner request (2026-09-27); the owner's answers are in. Not built yet. Tracked by AC-162
to AC-177 under [Gate R](../overseer-rfc.md#gate-r--voice-mode-added-by-the-owner-2026-09-27) in
the main RFC. The build is one goal: [voice-mode-goal.md](voice-mode-goal.md). The animation's
reference is [`docs/design/voice-mark/`](../design/voice-mark/index.html).

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
| Hard to interrupt | It does not interrupt easily. The owner's meaning (2026-09-27): other voice modes stop what they are doing at a tiny noise when nobody is even talking; Overseer must not. A higher tolerance, and reasoning about whether the owner actually meant to talk to it. See [Listening and the floor](#listening-and-the-floor). |
| Flow | It always flows and keeps listening to what is being said. |
| Quick answer | It quickly answers that it is working on the request. |
| Evidence | It shows what it is saying to every new agent and every in-flight agent it references for that request. |
| Audio | Audio is collected on the Rust side (owner, 2026-09-27, added while this RFC was written). No Swift or webview code captures the microphone. |
| The mark | The animated Overseer logo is in the middle while the conversation is happening. The waveform has an effect on it, so the owner knows Overseer is getting what is said. |
| The animation | Star (owner, 2026-09-27, from the preview page: "perfect"). It replaces the earlier pick of Orbit. See [The mark in the middle](#the-mark-in-the-middle). |
| Working while listening | The mark reacts to the owner's real voice, from the Rust listener, while Voice Mode listens: not a simulation. |
| Open floor | Fine (owner, 2026-09-27). |
| Who is spoken to | A setting: Overseer (the orchestrator, the default) or one agent. See [Talking to one agent](#talking-to-one-agent). |
| Redirects | A spoken redirect goes out without a yes, after the 2 s window in which it can be cancelled. |
| Permissions by voice | Yes, one at a time, after Overseer reads the request back. The owner must be sure it was taken: a sound (a Reactor key, under Audio Mode's rules: on and off, and the key for the clip) and a toast, and it can be cancelled. See [Answering a permission by voice](#answering-a-permission-by-voice). |

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
- **Name first and push to talk** stay as settings for shared rooms; the open floor is the default.
- **The typed chat keeps its own rule.** A spoken redirect needs no yes (the owner's decision);
  what is typed follows Overseer's level (Gate S, AC-186).
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
- The listener sends **words** and **levels** for the mark, never the recording, to the daemon
  that started it.
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

The owner's complaint about other voice modes (2026-09-27): they stop what they are doing at a
tiny noise when nobody is even talking. So Overseer has a high tolerance at two levels. The sound
itself must be speech (the speech gate), and the words must be meant for Overseer (the intent
check). Noise alone never changes anything: not the mark, not Overseer's voice, not an agent.

| Who interrupts whom | Rule |
| --- | --- |
| Noise, or the owner, interrupts Overseer's speech | Overseer keeps talking unless the owner really speaks to it. |
| Overseer interrupts the owner | Never. It waits for the end of the thought and for a free floor. |
| Overseer interrupts an agent | Only when the request changes what the agent is doing now. See [Sending](#sending-add-redirect-stop). |

### The speech gate

The listener decides on the Mac, from the sound alone, whether someone is speaking. Only then does
the mark move, is Overseer's voice lowered, or is anything sent to the recognizer.

| The sound | Counts as speech |
| --- | --- |
| At least 250 ms of voiced sound in the speech band, 12 dB above the room's noise floor | yes |
| Taps, clicks, typing, a chair, a door, a cup set down (short and broadband) | no |
| A cough, a laugh, a sigh (no voiced syllables) | no |
| A steady hum, a fan, music in the background | no: the noise floor follows it |
| Overseer's own voice and Audio Mode's cues through the speakers | no: cancelled first |

The numbers are the starting point; the spike (AC-162) sets them on the owner's Mac against real
rooms, and a change is recorded here.

### Meant for Overseer

Speech that passes the gate is not yet a request. With the open floor the owner also talks to
people, takes calls and thinks aloud. An utterance counts only when it reads as meant for the one
being spoken to (Overseer or the chosen agent): it names Overseer or an agent, continues the
conversation, answers a question Overseer asked, or asks for something about the agents. Anything
else (talk to someone in the room, a call, a video) makes no request, no answer and no sound; it
stays in the rolling context only. Words that are clearly addressed (a stop word, Overseer's name)
are decided on the Mac at once; the rest is the orchestrator's first judgement, before it writes
anything, and costs no answer when the answer is no.

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

None of these starts a request, lowers or stops Overseer's speech, or moves the mark:

- anything the speech gate rejects: noise, typing, music, a cough, a door;
- sound that does not become words;
- backchannels;
- speech that is not meant for Overseer;
- Overseer's own voice and Audio Mode's cues coming back through the microphone (the listener
  cancels the Mac's own output).

### When the owner speaks over Overseer

| The owner | Overseer |
| --- | --- |
| makes a noise, coughs, types | keeps talking at full voice |
| says one word or a backchannel | keeps talking |
| says two or more words (recognized, not a backchannel) | lowers its voice within 150 ms of the second word and keeps listening (revised by the spike: within 0.35 s of the second word, about 0.6 to 1 s after the owner starts, since lowering needs recognized words) |
| is still speaking 0.7 s later, and the words are meant for Overseer | stops at the end of its phrase |
| is still speaking, and the words are not meant for Overseer | returns to full voice and finishes |
| says "stop", "wait" or "hold on" | stops within 300 ms |

Lowering needs words, not sound, and stopping needs words meant for Overseer. That is what makes it
hard to knock over and still quick to yield. What Overseer had not said yet stays in the card.
"Go on" says the rest.

### When Overseer wants to speak

- It never starts speaking while the owner is speaking.
- A line waits for a free floor. After 20 s it goes to the card alone.
- Listening never stops: not while Overseer thinks, and not while it speaks.
- More words from the owner inside the settle window join the same request. Later words start a
  new request that knows about the previous one.

### Who is being addressed

| Setting | Behaviour |
| --- | --- |
| Open floor (default, confirmed by the owner) | Every utterance is heard; only what is [meant for Overseer](#meant-for-overseer) counts. |
| Name first | Saying "Overseer" or an agent's name opens the conversation. It stays open during the exchange and for 60 s after. |
| Push to talk | Overseer listens while a key is held. |

Open floor is what the owner asked for. Its cost is stated under
[Privacy and security](#privacy-and-security): the words of speech that passes the gate reach the
orchestrator's model for the intent check, even when they turn out not to be meant for it.

### Talking to one agent

Who the owner is talking to is a setting (`voice.target`), switched in the voice strip, by a
command, or by voice ("talk to Continuity", "back to Overseer"):

| Target | What happens to what is said |
| --- | --- |
| Overseer (default) | The orchestrator works out the agents from context, as below. |
| One agent | What is meant for it goes to that agent as its next message, with the same card and evidence, and no working out of targets. Naming Overseer ("Overseer, stop everyone") still reaches Overseer. |

The strip always shows the target, and the mark's view names it. When the chosen agent finishes or
is archived, the target returns to Overseer and Overseer says so once.

### Mute and calls

- One global shortcut and one command mute Voice Mode. Muted means the microphone is closed and
  macOS's own indicator goes off. It does not mean "listening but ignoring".
- When another app starts using the microphone (a call, a recording), Voice Mode pauses by itself,
  says so in the strip, and resumes when the other app is done.

## The answer: heard, working on it, done

| Answer | What | Budget, from the end of the thought |
| --- | --- | --- |
| Heard | A soft signal; the words appear in the voice strip | 300 ms (p95) |
| Working on it | One spoken sentence: what will be done and for whom | starts within 1.5 s (p50), 2.5 s (p95); revised by the spike: "On it." at once, the plan when it comes ([the spike](#the-spike-measurements-and-decisions)) |
| Holding line | "Working on it.", once, if the sentence above is late | at 2.5 s; revised by the spike: "Still working on it." at 8 s |
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

A request that starts or redirects an agent is read back before it goes, as the question it is:
"Start an agent in the site repo to draft the page's sections?" The settle window is then as long
as the read-back takes to say, plus the setting, so the owner hears all of it and can still correct
it (AC-229). The recognizer is given Overseer's vocabulary (repo, agent, merge, worktree…), the
agents' titles and the repositories' names, so "repo" is not heard as "rebuild". A request that
names Overseer and gives it an instruction is Overseer's whatever its own judgement says: when it
answers "not for me", the daemon asks it again, once, saying so.

## What voice may do

The daemon enforces the tiers, whatever the plan says.

| Tier | Actions | How |
| --- | --- | --- |
| Look | Questions about the agents; select or track an agent; open the grid; mute | At once |
| Steer | Add, redirect, stop an agent or every agent; up to three new agents | Answer, settle window, send. Stop skips the window |
| Confirm | Set an agent to Auto or start one in Auto; answer a permission request; merge back; open a pull request; archive; more than three new agents | Read back in one sentence, then a yes by voice or click within 20 s. Silence or anything unclear is a no. A permission answer then plays its cue, shows its toast and waits out the settle window, where it can be cancelled (below) |
| Not by voice | Accounts and sign-in; phone access and pairing; Continuity's download and install settings; workspace cleanup; stopping the daemon; changing these rules | Overseer opens the place in the UI and says so |

Permission requests are answered one at a time. "Allow everything" is refused.

### Answering a permission by voice

The owner decided (2026-09-27) that permissions may be answered by voice, and that there must be no
doubt it was taken.

1. Overseer reads the request back in one sentence: "Codex wants to run `npm install` in
   overseer. Allow?" It does so by itself the moment the request starts to wait (AC-230), with
   no need to ask; the conversation shows the same question with Yes and No.
2. The owner answers. Only a clear yes or no counts; silence for 20 s or anything unclear is no
   answer, and the request stays waiting.
3. At once: a sound and a toast. The sound is an Audio Mode cue, so it follows Audio Mode's rules:
   it plays only while Audio Mode is on, through the audio arbiter, and uses a Reactor key
   (`agent_unblocked` for allow, `agent_stopped` for deny); no sound is added. The toast shows
   whatever Audio Mode is set to: *Allowed: npm install for Codex* with **Cancel**, and a bar
   for the time left.
4. The answer is held for the settle window (2 s). "Cancel", "no, wait" or the toast's Cancel
   withdraws it: nothing reaches the agent, the request is waiting again, and the toast says so.
5. After the window the answer goes to the agent once, and the toast reads *Sent*.

## Audio Mode and Voice Mode together

One audio arbiter in the daemon decides what plays.

| Situation | Rule |
| --- | --- |
| A cue is due while Overseer speaks | It waits for the end of the phrase. |
| A routine cue is due while the owner speaks | It is dropped. |
| An attention cue is due while the owner speaks | It waits for the end of the thought, at most 5 s. |
| The heard signal | It reuses one of the twelve Reactor keys. No sound is added or generated. |
| A permission answered by voice | `agent_unblocked` (allow) or `agent_stopped` (deny), like any cue: only while Audio Mode is on, and through this arbiter. The toast shows either way. |

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
- **Levels.** What moves the mark is one number, how loud, sent only while the speech gate is
  open. It cannot be turned back into speech, and it is never stored.
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
| `voice.target` | Overseer | Overseer, or one active agent |
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
| `voice.level` | The loudness of the owner's voice (while the speech gate is open) or of Overseer's; at most 30 a second; not stored |

| Record | Fields |
| --- | --- |
| `VoiceRequest` | id, heard at, words, kind, tier, state, settle until, spoken answers, superseded by |
| `VoiceDispatch` | request, run or new task, reason, confidence, delivery and its reason, message, hash, state with times, event ids |

## The mark in the middle

While a voice conversation is happening, the Overseer mark (AC-142) is in the middle and moves
with the voice. It answers one question at a glance: is Overseer getting what I am saying?

The owner picked the **Star** animation (2026-09-27). The swooshes never move; the life is in the
star at the core and the glow behind the mark. Its reference implementation, with every number in
one table (`MOTION`), is [`docs/design/voice-mark/index.html`](../design/voice-mark/index.html):
open it in a browser to see each state. The build ports its `MOTION`, `step()` and `pose()` as
they are, so the product and this table cannot drift.

| State | Star | Glow behind the mark | Core |
| --- | --- | --- | --- |
| Listening (on, nobody speaking) | at rest | faint (18%), breathing slowly (±5% over 4 s): it is on | still |
| Hearing you | grows with the owner's voice, up to 1.85× its size, and brightens | swells with the voice, up to 98%, and widens | still |
| Thinking | turns slowly (1.3 rad/s) and sits 12% larger; comes to rest on a quarter turn, where it looks as drawn | faint | still |
| Speaking | grows with Overseer's voice, up to 1.3×, and brightens | swells, up to 53% | a ring of light leaves the star on each stressed syllable (level above 0.42, at least 200 ms apart) and fades at the rim in 0.95 s |
| Muted | at rest | none | the whole mark grey and dimmed to 45%, with a mute sign |
| Paused for a call | as muted, with a pause sign | | |

- **Timing.** The mark starts to move when the speech gate opens (within 300 ms of the first
  word), then follows the voice within 100 ms: it rises in about 30 ms and falls back over about
  0.5 s. Changes between states blend over about 120 ms.
- **Colour.** The glow is the theme's accent; the ring of light is the star's own white. Nothing
  else is tinted.

- **A still mark means it does not hear.** Noise that does not count as speech leaves the mark
  calm. So when the owner speaks and the mark does not move, Overseer is not getting it.
- **Where.** In the centre of the voice view, which takes the middle of the editor area like the
  home chat (AC-72). Beside a review or the grid the same mark is shown small in the voice strip,
  so the work is not covered.
- **Hearing grows the star; speaking sends light out of it.** The two are told apart by motion
  (a swelling star, or rings crossing the core), not by colour.
- **Levels, not the recording.** The mark is drawn by the windows, and the sound is heard by the
  Rust listener, so something has to travel from one to the other: one number, how loud, at most
  30 times a second, only while the speech gate is open. Speech cannot be rebuilt from it, and it
  is never stored. Every window draws from the same levels.
- **States without colour alone.** Each state differs in motion and shape, so it reads in
  grayscale and for colour-blind eyes.
- **Reduced motion.** With reduced motion on, the mark is still and a small level meter shows the
  voice.
- **Cheap.** 60 frames a second without slowing the views beside it. Nothing is drawn while the
  view is hidden.

**How it was chosen.** The owner left the animation to the implementing agent's judgement, with
one suggestion: a gradient moving against the mark. Five candidates were drawn on the real logo,
with a simulated voice, on a [preview page](https://claude.ai/artifact/7YePXA48Ht7CoBAtYJuyWr). The owner first picked Orbit, then Star.

| Candidate | The effect | Needs |
| --- | --- | --- |
| Gradient | A gradient moves through the mark: its colours turn, and a band of light sweeps across, faster and brighter with the voice. The owner's suggestion. | The logo as it is |
| Ring | The mark stays as drawn. A ring around it carries the waveform of the voice. | The logo as it is |
| Orbit | The swooshes turn around the core, faster and a little wider with the voice. | The layers |
| Star | The star grows and the glow behind the mark breathes with the voice. | The layers |
| Together | Orbit, the gradient on the swooshes, and the star, at once. | The layers |

With Star the swooshes stay still, so the small flaws on the inner edge of the swoosh layer (see
below) never show.

**The layers.** The owner's logo is one transparent image. At the owner's suggestion a copy was
cut into three layers by a script: the core as a whole disc, the swooshes as one ring, and the
star. They are in [`docs/design/brand/layers/`](../design/brand/layers/), with the script. The
owner's file is untouched. The [brand notes](../design/brand.md#the-mark-in-layers-docsdesignbrandlayers)
give the numbers and the limits. Separating the three swooshes from each other by machine was
tried and does not work (one colour gradient around the ring, no 120° symmetry); Star does not
need it.

Effects on the mark are allowed. The brand notes once forbade them; the owner corrected that on
2026-09-27. What stays fixed is the mark's shape and proportions.

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
| Levels for the mark | one number, 30 a second at most; never stored |
| While off | no listener process, microphone closed, no recognizer loaded |

## Working alongside the other gates

| Gate | Relation |
| --- | --- |
| Gate S, Overseer itself (AC-180 to AC-202) | Gate S builds the Overseer session in the daemon (AC-181); Voice Mode is its spoken side. Its classes are Voice Mode's (AC-185), its settle window is AC-170's, and its level (AC-186) leaves what the owner says by voice to this gate. The two are built in parallel, and whichever comes first builds the session. |
| Gate O, Audio Mode | One arbiter; Audio Mode is unchanged. |
| Gate L, Continuity | The orchestrator follows its failover and its memory budget. A speech model counts against the same budget. |
| Gate N, phone | No voice on the phone. Cards appear there when the chat with Overseer does (AC-128). |
| Swarm and Auto | New agents go through them once they are merged. |
| Main RFC | "No embedded inference model" is revised for speech only: a recognizer turns speech into words on the Mac and decides nothing about agents. |

## Testing

Everything that does not need the owner is built and tested first, with a simulated voice (owner,
2026-09-27). The owner is needed only for what a simulation cannot prove: the microphone prompt,
a real room, their real voice, and the session.

- **Words layer.** Most tests send fixture words with timings through `voice.say` and use a
  fixture orchestrator that returns scripted plans. They are deterministic and cost nothing.
- **Audio layer.** The listener reads PCM audio (16 kHz, 16-bit, mono) from a file or from its
  standard input instead of the microphone, at real-time pace or as fast as it can. Test speech
  is synthesized at test time (`say -o <tmp>.wav --data-format=LEI16@16000 "…"`) into a temporary
  folder and deleted; noise (taps, clicks, typing, a chair, a cough, a fan, music) is generated by
  the tests. The repository holds no recorded or generated voice, as Audio Mode requires.
- **A simulated voice for a running daemon.** For building and screenshotting the voice view, the
  daemon can be fed simulated audio or words while VS Code is open, so the mark can be seen in
  every state (listening, hearing, thinking, speaking with Overseer's own voice, muted, paused,
  reduced motion). This path exists only when the daemon starts with `OVERSEER_VOICE_SIMULATE=1`,
  and never from a phone.
- **Live.** A small sample on the default account inside the paid-turn budget, one attempt per
  step, recorded with its misses.
- **By voice.** The owner's checks: the microphone prompt, a quiet room (AC-164), the star
  following their real voice (AC-177), echo on the real speakers (AC-162), and the session
  (AC-176).

## Limits and out of scope

- Telling the owner's voice from another person's.
- Voice on the phone; voice from outside the room.
- Online or streaming voices; cloned voices.
- Dictating code, or reading an agent's output aloud in full.
- Messaging a harness's native child directly.
- Linux and Windows: they report Voice Mode as unavailable.

## The owner's answers

| Question | Answer (2026-09-27) |
| --- | --- |
| 1. What does "not interrupt easily" mean? | Other voice modes stop at tiny noises when nobody is talking. A higher tolerance, and reasoning about whether the owner meant to talk to it: [the speech gate](#the-speech-gate) and [meant for Overseer](#meant-for-overseer). |
| 2. Open floor, or say "Overseer" first? | Open floor is fine. Who is spoken to, Overseer or one agent, is a setting: [talking to one agent](#talking-to-one-agent). |
| 3. May a spoken redirect go out without a yes? | Yes, after the 2 s window to cancel. |
| 4. May permission requests be answered by voice? | Yes, one at a time, after the read-back; with a cue under Audio Mode's rules and a toast, and cancellable: [answering a permission by voice](#answering-a-permission-by-voice). |
| 5. Which animation for the mark? | Star ([preview page](https://claude.ai/artifact/7YePXA48Ht7CoBAtYJuyWr)). The large mark in the voice view and the small one in the strip are fine for now; the owner judges again on the build. |

These defaults stand without an answer: the system voice first, picked by ear later; Overseer
speaks only in answer (Audio Mode's cue says when an agent needs the owner); the typed chat keeps
Gate S's level; and the listener is a Rust process of its own, started by the daemon.

## The spike: measurements and decisions

AC-162, measured on files and with speech made by macOS at test time (no microphone yet; the
owner's checks are below). Machine: Mac17,6, Apple M5 Max, 128 GiB, macOS 26.6.2 (25G83), with a
load average of about 300 from other agents throughout, so the times are upper bounds. Versions:
whisper-rs 0.16.0 (whisper-rs-sys 0.15.0, whisper.cpp with Metal), the ggml English models pinned
by SHA-256 (small.en `c6138d6d…`, 487,614,201 bytes; base.en `a03779c8…`, 147,964,211 bytes),
Claude Code 2.1.246 with Claude Haiku for the orchestrator's timing.

**Recognizer.** 36 utterances (12 sentences in three system voices: Daniel, Eddy, Flo), with and
without the vocabulary hint (agent names and Overseer's words, given to the model as a prompt).

| Model | Load | Per utterance: median, p95, max | Per second of speech | Word error rate | Peak memory |
| --- | --- | --- | --- | --- | --- |
| tiny.en | 19,484 ms (cold) | 58, 184, 220 ms | 37 ms | 26.4% | 215 MiB |
| tiny.en + hint | 714 ms | 112, 205, 278 ms | 50 ms | 21.1% | 212 MiB |
| base.en | 635 ms | 327, 618, 654 ms | 147 ms | 19.1% | 297 MiB |
| base.en + hint | 402 ms | 42, 63, 65 ms | 18 ms | 13.0% | 305 MiB |
| small.en | 6,648 ms (cold) | 137, 191, 196 ms | 60 ms | 15.4% | 714 MiB |
| **small.en + hint** | 328 ms | 148, 221, 259 ms | 66 ms | **4.5%** | 708 MiB |

Words so far (a partial result) take 122 ms for 1 s of speech and 138 ms for 2 s with small.en
and the hint. The words recorded from it are the words-layer fixture
`voice/tests/fixtures/words-small-en.json` (text only), replayed by the daemon's tests.

**Orchestrator.** The first sentence from Claude Haiku through Claude Code takes 4.69 s in a new
session (3.40 s of it the API) and 3.87 s when resumed (2.08 s); a whole Overseer turn in Gate S
takes about 10 s.

**Speech gate.** Ten kinds of noise, 100 times each while nobody speaks and while Overseer speaks
(3,000 in all including its own voice), opened the gate 0 times. Over 30 spoken sentences in five
voices the gate opened after a median of 155 ms (p90 299 ms, at most 434 ms); the level for the
mark averaged 0.44 while speaking.

**Decisions** (the budgets above are revised where marked):

1. **small.en with the hint** is the recognizer: a quarter of base.en's errors at 148 ms an
   utterance. It is loaded only when Gate L's memory budget has room (852 MB, whisper.cpp's
   figure; checked before the listener starts). base.en stays selectable.
2. **"On it." at once, from the daemon.** The orchestrator's first sentence (3.9 to 4.7 s) misses
   the 1.5 s budget, so the daemon says "On it." when it takes the request, with no model; the plan
   line ("Telling Phone…") is spoken when the proposal comes, and "Sent." from the daemon's own
   records. The holding line moves from 2.5 s to 8 s ("Still working on it.", once): at 2.5 s it
   would follow "On it." every time.
3. **Lowering needs words**, so it comes within 0.35 s of the owner's second word (words are asked
   for every 0.3 s while Overseer speaks), about 0.6 to 1 s after the owner starts, not 150 ms after
   the second word; the listener's test measures 240 ms. Stop words still stop it within 300 ms of
   the words.
4. **The gate opens within 300 ms for 9 in 10, 450 ms at most.**
5. **Overseer's voice is made in memory** with macOS's speech synthesizer writing into callbacks,
   not with `say` into a file, so no audio is written at all, the owner's or Overseer's. `say` is
   used only by tests, to make speech at test time in a temporary folder.
6. **Audio held is 30 s at most, an utterance 90 s:** past 30 s the first 25 s of a long utterance
   is turned into words and let go, and the words are joined at the end.
7. **Calls** are found in Core Audio's list of processes that record (macOS 14.2 and later): any
   process but the listener that records pauses Voice Mode within 2 s.
8. **The GPU's shader cache.** Loading the model on Metal writes macOS's shader cache
   (`com.apple.metal` in the user cache folder) and opens the listener's own folder for writing;
   neither holds audio. The offline test allows those two and checks the cache for audio.

Still to measure with the owner (step 4): echo cancellation through real speakers
(VoiceProcessingIO), the microphone prompt naming Overseer, and the times from a real microphone.

## The owner's checks

What the simulated voice cannot show. About 20 minutes on the owner's Mac, plus the session (AC-176).
Everything else in Gate R is tested with the simulated voice and fixtures.

1. **In a dev daemon (Gate T's guided test):** an isolated dev Overseer beside the installed one
   ([dev-instance.md](dev-instance.md)); nothing is installed into the owner's own VS Code. The owner
   says "let's start the voice mode test" and an agent opens it and walks through the steps below.
2. **Turn it on (AC-163):** ⌘⌥⇧V (*Overseer: Voice Mode: Turn On or Off*). Accept the speech model
   download (small.en, 465 MiB, once). macOS asks for the microphone: the prompt must name
   **Overseer Listener**. Allow.
3. **Mute (AC-163):** ⌘⌥⇧M. The orange microphone dot in the menu bar goes off within a second. ⌘⌥⇧M
   again to unmute.
4. **The mark (AC-177):** ⌘⌥V shows the voice view. Talk normally: the star grows and swings with
   the voice. Tap the desk, type, cough: it stays at rest.
5. **A quiet room (AC-164):** ten minutes of ordinary work (typing, moving, a video playing) with
   Voice Mode on and nobody talking to Overseer: no request, no interruption.
6. **Echo on speakers (AC-162):** on speakers, not headphones, say "what's running?". Overseer
   answers aloud and its own voice does not come back as a request. While it talks, say
   "Overseer, stop": it stops at the end of the phrase.
7. **VS Code closed (AC-163):** quit VS Code and say "Overseer, what's running?". It answers aloud.
8. **The session (AC-176):** real work by voice for a while; then the date, what worked, what did
   not, and the friction points.

## Order of work

One goal: [voice-mode-goal.md](voice-mode-goal.md), in two phases (owner, 2026-09-27). First
everything that does not need the owner, with a simulated voice. Then the owner's checks, while a
loop every 5 minutes keeps the pull request in step with the branches it depends on.

It is built in its own worktree and pull request, branched from Gate S's (pull request #14) and
based on it. Only steps 2 and 3 need Gate S's Overseer session, but Voice Mode changes the same
files (`daemon.rs`, `server.rs`, `audio.rs`, the home view), so starting from it avoids a large
conflict later. The pull request is retargeted to `main` when #14 merges.

| Step | Criteria | Outcome |
| --- | --- | --- |
| 0. Find out | AC-162 | The spike: audio capture in Rust, the recognizer, echo cancellation, the microphone permission, the orchestrator's speed. Decisions written here. What needs no macOS prompt is measured on files first. |
| 1. Hear | AC-163, AC-164, AC-172, AC-173, AC-177 | The Rust listener, the voice session in the daemon, the speech gate, the arbiter, privacy and bounds, and the voice view with the Star mark in every state, driven by simulated audio. Nothing is sent to agents yet. |
| 2. Answer and send | AC-165, AC-166, AC-167, AC-168, AC-171 | The three answers, targets from context, delivery, new agents, the tiers. |
| 3. Show and prove | AC-169, AC-170, AC-174, AC-175 | Cards and messages, correcting and cancelling, permission answers with cue and toast, the UI, failures. |
| 4. The owner | AC-162, AC-164, AC-176, AC-177 | The microphone prompt, the live checks and the session, while the 5-minute loop keeps the pull request current. |

## Acceptance

AC-162 to AC-177 in the main RFC are the acceptance criteria. Each has its Verify clause there.
