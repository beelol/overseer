# Prepared goal: build Voice Mode

Status: prepared on 2026-09-27, not started. Scope: AC-162 to AC-177 under
[Gate R](../overseer-rfc.md#gate-r--voice-mode-added-by-the-owner-2026-09-27) in the main RFC,
following the [Voice Mode RFC](voice-mode.md). The owner's answers are in the RFC; the mark's
animation is [`docs/design/voice-mark/index.html`](../design/voice-mark/index.html).

The goal command takes at most 4,000 characters. The text below has 3,908. Paste it as it is.

## Goal text

```text
Build Voice Mode (Gate R, AC-162 to AC-177) in beelol/overseer: a voice the owner talks to constantly that redirects every orchestrated agent from context. Design, the owner's decisions and the animation's per-state table: docs/rfcs/voice-mode.md. Criteria and Verify clauses: Gate R in docs/overseer-rfc.md. Follow AGENTS.md.

Known state on 2026-09-27. Fetch and check; do not trust it.
* Nothing of Voice Mode is built.
* Gate S (PR #14, branch claude/orchestrator-agent-control-rfc-8e2009, finished, marked ready) builds the Overseer session in the daemon. Voice Mode is its spoken side and changes the same files (daemon.rs, server.rs, audio.rs, the home view). Branch from #14's head and open a draft PR based on that branch (open-pr skill); retarget it to main when #14 merges. Never push to #14 or claude/gate-s-fixes.
* docs/design/voice-mark/index.html is the Star animation. Port its MOTION, step() and pose() unchanged; draw from docs/design/brand/layers/.
* Keep the RFC's decisions: a Rust listener the daemon starts as its own process, sending words and one loudness level, never the recording; noise never interrupts (speech gate plus intent check); open floor with voice.target; redirects without a yes after the 2 s window; permissions by voice with read-back, cue, toast and cancel; no voice on the phone.

Phase 1: everything that does not need the owner, with a simulated voice
* The listener reads 16 kHz PCM from a file or stdin instead of the microphone. Tests make speech with `say -o <tmp>.wav --data-format=LEI16@16000` and generate noise (taps, typing, a cough, a fan, music), then delete it. Commit no recorded or generated voice.
* Words: voice.say with fixture words and timings, a fixture orchestrator, a fixture listener.
* With OVERSEER_VOICE_SIMULATE=1 a running daemon takes simulated audio or words, so the VS Code voice view is built and screenshotted in every state: listening, hearing, thinking, speaking (Overseer's voice through the arbiter), muted, paused, reduced motion.
* Order: AC-162's spike on files (Rust capture, the recognizer on synthesized speech, speed, memory; nothing that raises a macOS prompt); then AC-163, AC-164, AC-172, AC-173, AC-177; then AC-165 to AC-171, AC-174, AC-175 on Gate S's session. Record each owner-free part, and name the owner's part in the record.

Phase 2: the owner, and keeping in step
* Ask once, with exact steps, for what only the owner can do: the microphone prompt, AC-164's quiet room, AC-177's real voice, AC-162's echo on the real speakers, AC-176's session. Do not wait on the answer.
* Then start a 5-minute loop (the loop skill or a cron task, never a foreground poll): fetch main and #14's branch; if either moved, merge it in, resolve conflicts so both sides work, run the fast tests, push, refresh the PR description. Act on the owner's replies as they come. Stop when #14 is merged, the PR targets main, and every criterion is verified or waits only on the owner.

Rules
* Code on the PR branch. Criteria, RFC and ledger (docs/verification/records.py, explicit commit=) on main; fetch main right before each push, since AC numbers move within minutes.
* Push after each criterion; merge, never rebase or force-push. Mark the PR ready when done; the merge monitor merges it (AC-146).
* Paid turns: Claude haiku, light; ChatGPT only gpt-5.6-luna at low effort; one attempt per step, no retry loops.
* At the end leave no test windows, daemons, listeners or loops running. Restart the owner's daemon only when no runs are active.

Verify, serially
cargo test --workspace
for f in test/unit/*.js; do node "$f"; done
node extension/scripts/package.js, then each test/ui/scenario-*.js you add or touch
scripts/test-all

Check a box only when its whole Verify clause is covered; otherwise record it partial with the gap. Finish with the PR link, exact test totals, and each Gate R criterion's status with its blocker.
```

## Why this shape

| Choice | Reason |
| --- | --- |
| Owner-free first, with a simulated voice | The owner asked to get as much done as possible without them (2026-09-27). Simulated audio proves the speech gate, the floor and every state of the mark; only the microphone prompt, a real room, the owner's real voice and the session need the owner. |
| Branched from Gate S's pull request | Steps 0 and 1 do not need Gate S, but steps 2 and 3 use its Overseer session, and Voice Mode changes the same files as #14 (8,000 lines, finished). Starting from it avoids one large conflict later. |
| A loop every 5 minutes afterwards | #14 and main keep moving (the merge monitor has fixes for #14 on `claude/gate-s-fixes`). The loop merges them in as they land, so the pull request never drifts while the owner's checks are pending. |
| The reference animation ported unchanged | The owner approved the Star animation as it is on the preview page; porting its code keeps the product and the per-state table the same. |

## Before activating

- Nothing is required. The owner should expect one message listing the microphone prompt and the
  live checks, sent when phase 1 is done.
