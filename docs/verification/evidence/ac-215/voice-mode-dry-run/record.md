# Owner check: Voice Mode: the owner's checks

- Gate: R; criteria: AC-162, AC-163, AC-164, AC-176, AC-177
- Built from: main (f6b6a0534a85) in dev daemon dev-check-voice-mode
- Source: docs/rfcs/voice-mode.md#the-owners-checks
- Started: 2026-09-28T15:01:58.859Z; finished: 2026-09-28T15:03:41.066Z
- Run with: scripts/dev test voice-mode (AC-215)

## 1. In a dev daemon

- Asked: Is the dev window open as described, and your own VS Code untouched?
- Answer (2026-09-28T15:03:40.872Z): Agent verification run (no owner): the dev window opened, titled [dev-check-voice-mode] scratch, with the Overseer view and the two stand-in agents (one waiting for Write). Built from main f6b6a0534a85, which does not yet have stage 2's status-bar label, so the bar read 'Overseer 1 active'. The owner's VS Code was not touched.

## 2. Turn it on (AC-163)

- Asked: What did the microphone prompt say, and did Voice Mode turn on?
- Skipped (2026-09-28T15:03:40.897Z): not run: agent verification of the runner, not the owner's check

## 3. Mute (AC-163)

- Asked: Did the dot go off within a second, and come back on unmute?
- Skipped (2026-09-28T15:03:40.923Z): not run: agent verification of the runner, not the owner's check

## 4. The mark (AC-177)

- Asked: How did the star move with your voice, and did it stay at rest for the other sounds?
- Skipped (2026-09-28T15:03:40.947Z): not run: agent verification of the runner, not the owner's check

## 5. A quiet room (AC-164)

- Asked: Over the ten minutes, was there any request or interruption?
- Skipped (2026-09-28T15:03:40.971Z): not run: agent verification of the runner, not the owner's check

## 6. Echo on speakers (AC-162)

- Asked: Did it answer aloud without hearing itself, and stop at the end of the phrase?
- Skipped (2026-09-28T15:03:40.994Z): not run: agent verification of the runner, not the owner's check

## 7. VS Code closed (AC-163)

- Asked: Did it answer aloud with the dev VS Code closed?
- Skipped (2026-09-28T15:03:41.019Z): not run: agent verification of the runner, not the owner's check

## 8. The session (AC-176)

- Asked: The date, what worked, what did not, and the friction points.
- Skipped (2026-09-28T15:03:41.042Z): not run: agent verification of the runner, not the owner's check
