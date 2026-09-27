# Main reconciliation — 2026-09-27

Status: design review; no Swarm acceptance criterion is newly verified by this comparison.

Input: the latest published `origin/main` was fetched and checked with
`git ls-remote origin refs/heads/main`. Both named `7bccc3a9876382ac87a59f6cdeb392bb156ffa5c`.
`git merge-base --is-ancestor origin/main HEAD` returned success on the Swarm branch,
so main was already fully incorporated and no merge or rebase was needed. The owner's
local `main` checkout was 112 commits behind that published tip and had untracked
files; it was left untouched.

Changes relevant to Swarm since that local checkout:

| Main feature | Effect on Swarm |
| --- | --- |
| Gate S Overseer RFC, AC-180–202, including the owner's confirmed 100 self-started turns/day cap (`836c79c`) | Retain exactly one decision-maker inside each Swarm run: its director. Overseer can see and control the run or advise its director, never directly steer a worker. Share agent slots, account admission, area claims and the message broker with ordinary agents/watchers. The daily cap counts Overseer's own autonomous turns, not an approved director's turns; both still consume applicable account allowance. SWARM-24 now states this boundary explicitly. Joined AC-195/196 tests remain open. |
| Gate K/M sidebar, grid, chat and packaged UI; Audio Mode; `scripts/test-all` (`f4d6721`) | Keep Swarm in the existing agent tree and fixture UI suite. A grid tile or audio cue is a view of durable state, not a worker, admission or result receipt. The existing 100-job/32-worker packaged scenario exercises that tree, but real account usage and the binding limit remain unverified under SWARM-37. |
| Gate N phone protocol and its latest build-time choices (`759c080`) | The new choices concern phone rendering, pairing and notifications, not Swarm scheduling. Full control and Watch only scopes, request-id replay and daemon ordering remain the Swarm control contract; joined phone replay remains open under SWARM-20/61. |
| Gate R Voice Mode RFC | Voice is another input to the same daemon control path and Gate S conversation. It must not create a second director, admission authority or worker channel. Joined voice behavior remains unverified. |
| Recent live Claude evidence on main (`d1408a9`, `163140d`, `7bccc3a`) | It qualifies those base-product scenarios only. It does not qualify Swarm's director/worker communication, native descendant control, quota admission or cancellation, and is not counted toward SWARM-25. |

Decision: keep the separate opt-in Swarm toggle, one category director, adaptive worker
limits, frozen allocation and SWARM-01–64 scope. No worker-count default, budget policy,
scenario or acceptance box was weakened. The only new criterion wording is the Gate S
cap/admission boundary in SWARM-24. The separate Auto Mode branch still owns route and
usage observation; shared account-window admission remains an integration gap.

## Subsequent published main: `91c7fc8`

The Swarm branch merged `91c7fc8` after its seven new commits were fetched. They
changed Gate S owner decisions and review/keyboard UI code, with no Rust daemon
change and no merge conflict. Gate S now explicitly says Overseer never answers a
permission request on its own and gives its briefing/channel only when multiple
agents share a repository. The Swarm RFC and Auto contract now state how those
decisions apply: Gate S treats a category as one director agent; workers keep
their director brief and broker, and a permission request goes to the owner.
This preserves one decision-maker and the existing approved account/permission
scope. SWARM-24/60 and AC-190/195 need joined tests once Gate S lands; this
read-only design reconciliation verifies none of them. The review/keyboard changes
do not alter Swarm scheduling defaults or acceptance criteria.

## Subsequent published main: `80411ba`

The branch also merged `80411ba`, which adds AC-150/153 evidence, a link check,
and updated test/brand guidance in `AGENTS.md`. It changes no Swarm policy,
admission or worker behavior. `scripts/check-links` reports 612 links in 206
files with zero broken links on the merged branch. The current Swarm deadline
extension was exercised through the packaged UI after the preceding `91c7fc8`
merge; these later commits change no extension source or Swarm scenario code.
The full offline Rust suite passed before the main merges, which did not touch
Rust daemon code. The focused deadline fixture, extension source check, unit
control test, package build and packaged Swarm-status scenario passed after the
`91c7fc8` merge. No new acceptance box is checked by this reconciliation.
