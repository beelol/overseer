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
