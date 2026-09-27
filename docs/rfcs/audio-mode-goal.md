# Prepared goal: finish Audio Mode (terminal UI and the loose ends)

Status: done. Activated on 2026-09-26; T-23, T-24, AC-144 and AC-145 are verified, and pull requests #5 and #6 were merged by the owner on 2026-09-27.
Scope: T-23 and T-24 in the [TUI RFC](tui.md), and AC-144 and AC-145 under
[Gate O](../overseer-rfc.md#gate-o--audio-mode-added-by-the-owner-2026-09-26) in the main RFC,
following the [Audio Mode RFC](audio-mode.md). AC-143 is done in pull request #5 and is not part
of this goal, except that its evidence must still hold at the end.

The goal command takes at most 4,000 characters. The text below has 3,474. Paste it as it is.

## Goal text

```text
Finish Audio Mode. Make draft PR #6 (terminal UI) merge-ready and close the loose ends of draft PR #5, against T-23 and T-24 in docs/rfcs/tui.md and AC-144 and AC-145 in docs/overseer-rfc.md (Gate O), following docs/rfcs/audio-mode.md. Limit RAM, update the existing draft PRs, and do not merge.

Known state on 2026-09-26. Fetch and check; do not trust it.

* Repo: beelol/overseer. PR #5: codex/reactor-audio-mode into main, merge-ready, AC-143 verified at d7be0a3.
* PR #6: codex/audio-tui-controls (d67106a). It still targets claude/nostalgic-saha-9caaac, the branch of PR #4, which is merged into main. It needs the audio methods of PR #5.
* PR #6's full suite fails t19_a_new_waiting_agent_rings_and_shows_in_the_window_title. The test expects the bell in the same pass as the state; PR #6 rings only after the audio.get reply. T-24 says what is right. Fix the implementation; do not delete or weaken the assertion.
* GitHub runs no checks, so record local evidence.

Work

1. Use isolated worktrees. Leave the owner's checkout and unrelated files alone.
2. Bring PR #6 onto current main plus the head of PR #5 without losing main's TUI behaviour. Retarget it to codex/reactor-audio-mode, or to main once PR #5 is merged.
3. Implement T-23 and T-24. The TUI never plays sound and keeps no audio setting of its own.
4. AC-144: add its tests to daemon/tests/audio.rs on PR #5's branch. Change the daemon only where a test shows it disagrees with the criterion.
5. AC-145: build the VSIX from PR #5, write the owner's listening steps into the record, ask one precise question and continue other work. Its box stays unchecked until the owner confirms. Never read, copy or commit the owner's Commander recordings. Never regenerate a cue.
6. Test main, PR #5 and PR #6 together in a temporary tree.
7. Write records through docs/verification/records.py and TUI evidence where T-01 to T-22 keep theirs. Update docs/verification/audio-mode.md and both PR descriptions (open-pr skill) with current commits and actual results.
8. Criteria, records and RFC text go to main; code goes to the PR branches. Fetch main right before every push: criterion numbers there move within minutes.
9. Keep both pull requests draft.

Verification, run serially

CARGO_BUILD_JOBS=1 cargo test -p overseerd --offline -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test -p overseer-tui --offline -- --test-threads=1
npm run check --prefix extension
git diff --check

Also prove

* Every Verify clause of T-23, T-24 and AC-144, each by a named test or a recorded run.
* T-19's test passes as it is on main.
* In the combined tree with a real daemon: Audio Mode off gives one terminal bell and no cue; on gives one cue and no bell; a daemon without audio.get, or one that does not answer, gives the bell; the title and Needs you never wait for audio.
* PR #5 merges into current main without conflicts, PR #6 into its declared base, and the combined tree into main.
* The 12 MP3s still equal the approved pack: check-pack.py --approved with a folder the owner provides. The tools cannot read the voice lab's folder, so ask the owner for a copy they can read.
* Git tracks no WAV, Commander recording or generated voice file.

Check a box only when the whole criterion has evidence. If a live macOS check or an owner step cannot happen, leave the box unchecked and name the exact blocker. Finish with both PR links, exact test totals, merge status, and every criterion still unchecked with its reason.
```

## Why these criteria

| Criterion | What it settles |
| --- | --- |
| T-23 | The terminal UI can do what VS Code's Audio Mode menu does, through the daemon only. |
| T-24 | One signal per need: the daemon's cue or the terminal bell, never both and never neither. It also says why the failing T-19 test is right and pull request #6 is not. |
| AC-144 | The owner's decision that a lost session keeps the attention cue, with the test that was missing. |
| AC-145 | The owner hears the result. The checks so far prove which files are played, not how they sound. |

## Before activating

- Pull request #5 should be merged, or still merge cleanly into main. If main has moved, merge
  it into the branch first.
- The owner's approved pack must be readable by the session for the pack check: a copy made
  by the owner in a folder the tools can read.
- The goal needs the owner twice: for that copy, and for the listening session of AC-145.
