# Prepared goal: build Voice Mode

Status: prepared on 2026-09-27, not started. Scope: AC-162 to AC-177 under
[Gate R](../overseer-rfc.md#gate-r--voice-mode-added-by-the-owner-2026-09-27) in the main RFC,
following the [Voice Mode RFC](voice-mode.md). The owner's answers are in the RFC; the mark's
animation is [`docs/design/voice-mark/index.html`](../design/voice-mark/index.html).

The goal command takes at most 4,000 characters. The text below has 3,697. Paste it as it is.

## Goal text

```text
Build Voice Mode (Gate R, AC-162 to AC-177) in beelol/overseer: a voice the owner talks to constantly that redirects every orchestrated agent from context. Design: docs/rfcs/voice-mode.md. Criteria and Verify clauses: Gate R in docs/overseer-rfc.md. Follow AGENTS.md.

Known state on 2026-09-27. Fetch and check; do not trust it.
* Nothing of Voice Mode is built.
* Gate S (PR #14, branch claude/orchestrator-agent-control-rfc-8e2009) builds the Overseer session in the daemon (AC-181 verified). Voice Mode is its spoken side: reuse its session, roster, digests, action classes (AC-185) and cards. Build on main once #14 is merged; until then stack on its branch and say so in your PR. Never push to #14.
* Audio Mode is merged: Reactor cues and daemon/src/audio.rs.
* docs/design/voice-mark/index.html is the reference Star animation. Port its MOTION, step() and pose() unchanged and draw from docs/design/brand/layers/.

Owner decisions (in the RFC; do not reopen)
* Audio is collected in Rust: a listener crate the daemon starts as its own process. It sends the daemon words and one loudness level, never the recording; no audio is stored or leaves the Mac.
* Noise never interrupts. Other voice modes stop at a tiny noise when nobody is talking; Overseer must not. A speech gate on the sound and an intent check on the words (AC-164).
* Open floor; voice.target picks who is spoken to, Overseer (default) or one agent (AC-166).
* A spoken redirect goes out without a yes after the 2 s window to cancel it.
* Permissions by voice: one at a time after a read-back, a Reactor cue under Audio Mode's rules plus a toast, cancellable in the window (AC-171).
* The mark follows the owner's real voice while listening (AC-177). No voice on the phone.

Order: one worktree and one draft PR, opened early with the open-pr skill.
0. AC-162, the spike: capture in Rust, recognizer, echo cancellation, the microphone prompt in Overseer's name, orchestrator speed. Write the decisions into the RFC.
1. Hear: AC-163, AC-164, AC-172, AC-173, AC-177. First milestone: the voice view's Star mark follows the owner's real voice from the listener and stays at rest for taps and typing. Nothing reaches agents yet.
2. Answer and send: AC-165 to AC-168, AC-171.
3. Show and prove: AC-169, AC-170, AC-174, AC-175.
4. Confirm: AC-176 and AC-177's owner check.

Rules
* Code on the PR branch. Criteria, RFC and ledger (docs/verification/records.py, explicit commit=) on main; fetch main right before each push, since AC numbers move within minutes.
* Push after each criterion; never force-push; merge main into the branch and keep it out of conflict. Mark the PR ready when done; the merge monitor merges it (AC-146), not you.
* Paid turns: Claude haiku, light; ChatGPT only gpt-5.6-luna at low effort; one attempt per step, no retry loops. Most tests use fixture words (voice.say), a fixture orchestrator and a fixture listener.
* Test speech is made with `say` in a temporary folder and deleted; commit no recorded or generated voice.
* The owner is needed for the microphone prompt and the live checks (AC-164's room, AC-177's real voice, AC-176). Ask one precise question, keep working on the rest, never block on it.
* Leave no test windows, daemons or listeners running. Restart the owner's daemon only when no runs are active.

Verify, serially
cargo test --workspace
for f in test/unit/*.js; do node "$f"; done
node extension/scripts/package.js, then each test/ui/scenario-*.js you add or touch
scripts/test-all

Check a box only when its whole Verify clause is covered; otherwise record it partial with the gap. Finish with the PR link, exact test totals, and each Gate R criterion's status with its blocker.
```

## Why this order

| Step | What it settles |
| --- | --- |
| 0. Spike | The choices that are expensive to change: how audio is captured in Rust, which recognizer, echo cancellation, and how macOS asks for the microphone in Overseer's name. |
| 1. Hear | The owner's first visible result: the Star mark follows their real voice and ignores noise. Nothing reaches an agent yet, so the gate and the floor can be tuned safely. |
| 2. Answer and send | Voice starts to act on agents, through Gate S's classes and cards. |
| 3. Show and prove | The evidence the owner asked for: every word sent, and every answer cancellable. |
| 4. Confirm | The owner's session by voice. |

## Before activating

- Gate S (pull request #14) is best merged first, since Voice Mode reuses its Overseer session.
  The goal can start before that by stacking on its branch.
- The owner should be at the Mac for the spike's microphone prompt and for step 1's live check.
