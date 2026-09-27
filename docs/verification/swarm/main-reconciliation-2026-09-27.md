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

## Subsequent published main: `e01057f`

The branch merged main through `e01057f` after checking the published remote tip.
The two new commits add AC-159's Command Line Tools Git fallback to the UI
scenario harness and VSIX packager, and remove completed owner actions from the
README for AC-160. The fallback changes how qualification can run on a Mac whose
Xcode license is pending; it does not select a Swarm target, alter a worker's
environment, or count as a live Swarm run. The README change has no Swarm runtime
effect. Keep the existing Swarm RFC and SWARM-01–64 criteria unchanged. The
packaged Swarm-status scenario passed on this merged branch. Main records AC-159
as verified using its failing-Git fixture; that base-toolchain status does not
verify Swarm's live launch or account behavior. Neither
main commit supplies the Auto/Swarm shared admission transaction or resolves the
normal Swarm launch gap.

## Subsequent published main: `4fb5d60`

Main next recorded Gate Q AC-157–160 as verified, updated the AC-146/156 merge
and shared-agent-rule evidence, and revised the overall goal's fallback for
stalled agents. These are process and verification changes. Swarm keeps its
draft PR in flight, follows the shared `AGENTS.md` rules, and pushes this
reconciliation with the branch instead of treating a passing fixture as ready
for merge. No Swarm scheduling, permission, UI, or acceptance criterion changes:
the missing shared Auto admission path, normal launch, and joined Gate S checks
remain open. This merge provides no new SWARM verification evidence.

## Subsequent published main: `2ad4ef3`

Main next verified Gate Q's AC-152 load scenario and Gate S's AC-180 research
spike. AC-152 measures the base app, not Swarm's 100-job/32-worker behavior.
AC-180 identifies daemon-issued MCP tools and concrete read-only settings for
Claude Code, Codex and OpenCode. The Claude `Agent`/`Task` deny list and
OpenCode `task: false` are useful candidates for the Swarm worker descendant
control qualification; Codex's read-only sandbox still leaves its shell
available and does not by itself establish a native-delegation limit. None of
these Gate S probes ran a Swarm director/worker, enforced a total descendant
budget, or qualified a live Swarm account route. SWARM-17/25/52 remain open.
Keep one director, the current capability floor and the fail-closed live launch
boundary; consume Gate S's harness findings in the future per-harness
qualification rather than treating them as a new Swarm permission grant.

## Subsequent published main: `f8d5df6`

The branch merged main through `f8d5df6` after the owner's new push. Main now
records Gate S AC-183 (bounded agent digests/roster) and AC-192 (cross-agent
working-tree conflict detection) as **partial**, with their implementation and
fixture evidence on separate pull request #14. Those records do not put that
implementation on main or verify Gate S/Swarm AC-195. Swarm's current
`swarm.conflicts` records submitted-result conflicts within a run; they neither
replace the cross-agent detector nor prove its joined behavior. When Gate S's
daemon records land, Swarm should expose director and worker identity, claims
and conflict outcomes through that shared authority, and route a detected
cross-run overlap to the category director. Keep the existing one-director
boundary and SWARM-24/30/36/60 joined checks open. No new Swarm acceptance
criterion or worker-count default is needed.

The other main commit ports the live Codex/Claude conversation scenario to the
current Gate K chat and adds live Codex UI evidence. Future Swarm launch/readout
qualification should use that current surface, not legacy panel selectors.
It does not exercise a Swarm launch or change daemon scheduling. SWARM-01/25/39/63
remain unverified; no fixture result is promoted to live evidence by this merge.

## Subsequent published main: `b55f558`

The branch merged main through `b55f558`. Gate P now records AC-151 as partial:
in its live Claude Code 2.1.246 rerun, a background native subagent answered but
the parent did not finish the requested file and remained running until
interrupted. This is base-product evidence of a child/session lifecycle gap,
not a Swarm descendant-control qualification. It reinforces SWARM-17's existing
fail-closed rule: Claude cannot become an enabled Swarm target merely because
its children are visible; the selected launch must either disable native
delegation or enforce and recover every descendant within the run's limit.
No worker ceiling, budget default or acceptance box changes. Main's tracker
also now names the separate Gate S and Auto Mode PRs; those still need a joined
authority before live Swarm launch. This merge changes documentation and live
evidence only, so it does not invalidate the offline Start replay suite.

## Subsequent published main: `b3ea1e7`

The branch merged the latest published `origin/main` at `b3ea1e7` without a
conflict. This commit changes the ledger, README and Gate S verification
evidence; it adds no daemon or extension implementation to main. AC-181 now
records a daemon-owned Overseer conversation and AC-184 records bounded,
read-only Overseer tools as verified by fixtures and a packaged chat scenario
on Gate S pull request #14. The Swarm design already places Overseer above one
category director and requires shared daemon records. Once that Gate S code
lands, Swarm should attach its director summary and sourced advisories to the
existing conversation rather than start another one. A Swarm worker still has
no direct Overseer assignment channel, and Overseer's read-only tools still
grant no worker permission or account allowance. Joined SWARM-24/27/60 and
Gate S AC-195/196 remain open; no Swarm acceptance box changes.

The same ledger edit resets AC-151 to *not started* instead of reporting its
earlier partial live reruns. That administrative status change is not evidence
that the observed Claude background-child lifecycle issue was fixed. Keep
SWARM-17's version-pinned descendant-control qualification and fail-closed
target eligibility; rerun it against any enabled harness before live fan-out.

## Subsequent published main: `9667968`

The branch merged main through `9667968` without conflicts. Gate S now records
AC-185–189 as partial, with daemon implementation and fixture evidence on its
separate pull request #14. Those actions, holds, redirects and check-ins are
not yet on main. When they land, their target must be the Swarm director for
one category; worker directives still go through the director and durable
broker. Confirm actions, shared claims and an approved account pool remain
subject to the Swarm run's own authority. Joined SWARM-24/27/44/60 and Gate S
AC-195/196 are still open; the new ledger statuses verify none of them.

Main also fixed Claude Code 2.1.246's base run completion after a depth-2
background child or a notification read mid-turn (`06d6d75`) and records a
passing live Claude scenario under AC-151, which remains partial for other
app-server paths. The parser now counts only top-level background tasks for an
expected next turn and clears notices read by the main agent. Swarm's synthetic
Claude child fixture and its process/admission regressions passed after the
merge: `cargo test -p overseerd --test protocol --test swarm_runtime --test
swarm_admission --test swarm_dispatch --test swarm_director_process --bin
overseerd` (53 + 22 + 33 + 6 + 13 + 30 tests). This improves the base parser,
but it does not prove native delegation is disabled in a real Swarm worker or
qualify a live director/worker communication path. SWARM-17/25/31 remain open.
The audit-only native admission gate added at `8d9f328` continues to hold
Claude before reservation; there is still no qualified read-only source/service
boundary. No Swarm default or acceptance criterion wording changes.
