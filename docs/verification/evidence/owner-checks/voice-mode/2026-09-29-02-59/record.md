# Owner check: Voice Mode: the owner's checks

- Gate: R; criteria: AC-162, AC-163, AC-164, AC-176, AC-177
- Built from: main (ea0f1ebaeeac) in dev daemon dev-check-voice-mode
- Source: docs/rfcs/voice-mode.md#the-owners-checks
- Started: 2026-09-28T21:28:04.240Z; finished: 2026-09-29T02:59:57.146Z
- Run with: scripts/dev test voice-mode (AC-215)

## 1. In a dev daemon

- Asked: Is the dev window open as described, and your own VS Code untouched?
- Answer (2026-09-28T21:33:56.769Z): The dev window is open (title and status show dev-check-voice-mode, scratch repo, Overseer view). The first window opened transparent and click-through (the test harness's background launch reached owner checks); it was reopened as a normal window and scripts/dev was fixed so owner checks never use it.

## 2. Turn it on (AC-163)

- Asked: What did the microphone prompt say, and did Voice Mode turn on?
- Answer (2026-09-28T21:59:52.965Z): Voice Mode turned on after the model download and heard 'How's it going, Overseer?' correctly (request went to Overseer, no agent). Bugs found and fixed in the dev build: the dev instance lacked the listener; dictation (Wispr Flow, CoreSpeech) paused it as a call; a stale pause survived a listener restart. Overseer's turn then failed on an expired sign-in (the owner signed in twice). Owner feedback: no visible button to turn it on; 'Show voice' should become the home page turning into the voice view with animation; the voice choice is odd though clear; the request id 'V-0001' means nothing to a user; the line 'Stop, mute and what's running still work' is unneeded; Overseer should reason and talk back, not only dispatch.

## 3. Mute (AC-163)

- Asked: Did the dot go off within a second, and come back on unmute?
- Answer (2026-09-28T22:52:04.951Z): Yes: the orange microphone dot went off within a second of muting and came back on unmute. Separately, a natural follow-up (no 'Overseer', no command verb) was heard and transcribed but not sent: the local gate kept it as context. Owner wants follow-ups right after an answer to count.

## 4. The mark (AC-177)

- Asked: How did the star move with your voice, and did it stay at rest for the other sounds?
- Answer (2026-09-28T22:53:38.254Z): The star moves with talking. It also moves for coughs, and sometimes for desk taps, so noise is not fully filtered (AC-177's 'stays at rest' not met). Owner: noise filtering should be an optional checkbox.

## 5. A quiet room (AC-164)

- Asked: Over the ten minutes, was there any request or interruption?
- Answer (2026-09-29T00:29:03.269Z): Ten minutes of ordinary work with a video playing: two false requests from non-speech sounds. A burp became 'Go!' (V-0004, answered by Overseer), and a throat clear became 'and move.' (V-0005, left waiting and cancelled). Otherwise nothing, which the owner found not bad overall. Single command words heard from noise are sent (see AC-222). Also: Voice Mode lowered YouTube's volume while listening (macOS voice-processing ducking).

## 6. Echo on speakers (AC-162)

- Asked: Did it answer aloud without hearing itself, and stop at the end of the phrase?
- Answer (2026-09-29T00:32:03.799Z): Worked perfectly: on speakers it answered 'what's running?' aloud without hearing itself, and 'Overseer, stop' stopped it at the end of the phrase.

## 7. VS Code closed (AC-163)

- Asked: Did it answer aloud with the dev VS Code closed?
- Answer (2026-09-29T00:33:46.399Z): Yes: with the dev VS Code quit, 'Overseer, what's running?' was answered aloud (both agents done and inactive).

## 8. The session (AC-176)

- Asked: The date, what worked, what did not, and the friction points.
- Answer (2026-09-29T02:59:57.118Z): The owner's verdict: it did not fit their whole use case yet. It did not open the finished work when asked; it was hard to get it to show the agent; it should move the owner around VS Code (focus the agent asked about, open its chat or review, open the finished work). 2026-09-28, about 19:05-19:50, personal Claude on Haiku, two scratch repos (overseer-site, overseer-notes). Worked: agents started by voice in the named repo; Overseer asked a clarifying question ('what does rebuild refer to?') and took the follow-up; check-ins reported the agent done; echo and stop were solid earlier. Did not work: a clear request was judged 'not for Overseer' by the model; 'On it' was followed by 'not for Overseer'; 'repo' heard as 'rebuild' and 'sections' as 'actions', so the agent built the wrong thing with no read-back; no action to set an agent's permission mode ('auto mode' became a failed cadence change, raw error shown); a waiting permission did not come up by itself; after 'On it' nothing visible happened; Needs you did not clear when the agent was done; 'show me it' gave a relative path; the review showed 0 files and no diff for the agent's committed file, Save looked like accepting; Open PR on a repo with no remote gave a macOS alert. Friction and wishes: combine Voice Mode and Talk to Overseer into one view (Needs you inside it); slide the view aside to show a single started agent (with a setting to not auto-focus); cards take you to the agent or its work; open or edit an agent's worktree in place (a Follow option, Manual edit, with the agent's edits annotated); Overseer may set agents to Auto on its own. Only three agents in two repositories was not reached: one agent started.
