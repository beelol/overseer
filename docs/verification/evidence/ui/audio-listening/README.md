# The owner's listening session (AC-145)

`marks.json` is the record of the owner's session on 2026-09-27 (04:40 to 04:43 UTC), on the
build of pull request #5 (daemon `f7dc26e779c89859…`, the twelve cues with the hashes of the
approved pack). It was written by `test/ui/listen-audio.js` as it was then, which asked about
every step whether or not the session had done it. The table sets each answer beside the
session's own log (`heard` in the record).

| Step | Answer | What the session did | Counts |
| --- | --- | --- | --- |
| The twelve Reactor cues | all *Right* | each cue was played before its mark was asked | yes |
| Start, completion and attention with VS Code open | yes | Audio Mode on at 04:42:22, `s` at 04:42:23, `n` at 04:42:30; the window was open until the session quit | yes |
| With Audio Mode off, `s` and `n` were silent | yes | off at 04:42:47, `s` at 04:42:50, `n` at 04:43:03 and 04:43:06 | yes |
| Start, completion and attention with VS Code closed | yes | the window was never quit | no |
| Two agents at the same moment made one attention cue | yes | `t` was not pressed | no |
| System voice spoke with an installed voice | yes | `v` was not pressed; the track was Reactor throughout | no |
| The owner's own Commander folder played | yes | no folder was set (`commander_folder_set: false`) and `p` was not pressed | no |

The record is kept as the owner wrote it. AC-145 stays open until the four steps in the lower
half of the table have been heard.

Since then the script asks only about the steps a session did, records the settings and the
window's state with every event, and writes each session's record to its own file
(`marks-<date>-<time>.json`) beside the earlier ones.
