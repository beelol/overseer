# SWARM-24 — shared Auto Mode boundary

Status: partial. Support level: proposed contract plus Swarm fixture admission; no merged Auto producer or shared cross-mode account reservation transaction.

Input: compare this branch's `swarm.policy.preview` and `swarm.admit` with the separate `codex/automode-rfc` branch at `11b4cf9`, including its quota/route selector and durable `auto.dispatch` launch intent. Review current daemon `account.usage`, `account.list`, `harness.list`, and `profile.status` APIs. The contract request includes run/parent/job identity, attempt budget, native-unit upper estimates, and remaining category allocation; the transaction must apply the tighter account and category limits across every binding window.

Expected: ordinary, Auto and Swarm callers share one account-pool reservation and workspace/agent admission transaction. Stale observations and changed account identity force revalidation; a confirmed pre-effect failure can use remaining attempts, while an uncertain effect retains its commitment and cannot reroute. Contract proofs are shared with AUTO-AC-04/17/19/20/24, not separately certified.

Actual: Swarm fixtures revalidate injected snapshots, reserve their own fixture pool rows and share the app-wide agent-slot lock with ordinary starts. Auto's separate branch has structured quota observations, scoped route selection and selected launch intents, but its launch-boundary design explicitly leaves atomic allowance commitment and complete crash reconciliation open. Existing account APIs are observations, not the shared reservation ledger. The contract document records this boundary and the common acceptance-proof map. No adapter or shared cross-mode account transaction exists yet.

Evidence: `docs/rfcs/swarm-auto-contract.md`, `docs/rfcs/swarm-mode.md`, `daemon/tests/swarm_admission.rs`, and read-only comparison to `codex/automode-rfc:docs/verification/auto-mode/launch-boundary-design.md` at `11b4cf9`.

Remaining: merge or otherwise integrate the Auto producer behind a versioned adapter; implement the single shared admission authority and route-to-launch revalidation; run concurrent ordinary/Auto/Swarm pool and writer tests, changed identity, stale-known/unknown and crash/reconnect tests. Keep SWARM-24 unchecked.

Read-only update at Swarm `47c47a4` (2026-09-27): local Auto branch `d2e9d34` is ahead of its published `c769383` tip. Its route type carries harness, provider, endpoint, profile, pool, model and effort. Its durable launch boundary still describes measured-window admission and full crash reconciliation as unverified; the unknown-draw claim is not the shared numeric-window transaction required here. Swarm's fixture admission now carries harness, profile, model and optional effort into the worker launch, but it does not consume Auto's route producer or revalidate account generation/provider endpoint/model version. No Auto code was copied and no shared CONTRACT criterion changed status.

Observation-fence follow-up: fixture admission now compares its submitted snapshot with the latest persisted availability observation for that run. A newer, smaller eligible observation cannot be bypassed by submitting an older, larger snapshot; the stale request returns `snapshot_superseded` before reserving capacity. The current snapshot then reaches the shared-pool headroom check, and a later larger observation permits admission. Observations with the same timestamp but different allowance data conflict, and migrated eligible observations without snapshot identity become blocked until reobserved. `daemon/tests/swarm_admission.rs` (`newer_allowance_observation_fences_stale_admission_snapshot`) and the schema migration test pass. This is still fixture-owned evidence; the live Auto producer and common cross-mode reservation transaction remain open.

Main reconciliation at `7bccc3a` (2026-09-27): Gate S now confirms a cap of 100 Overseer-self-started turns per day. SWARM-24 adds a joined fixture requirement: exhaust that cap while an already approved category director is active, then prove the director continues inside its frozen allocation and that its turns do not increment Overseer's cap. Overseer's own turns and director turns must both still meter against their applicable account limits. This test is not implemented; the criterion remains partial. See `docs/verification/swarm/main-reconciliation-2026-09-27.md`.

Step 3 of the handover on `claude/auto-swarm` (`dc8b37aa`, 2026-09-27): Swarm admission of
an account target now books through Auto's shared booking (`book_shared_launch_in_tx`) inside its
own IMMEDIATE admission transaction, passing the category's remaining allocation in the booking's
own windows as `allocation_remaining_milli` (thousandths of a reported percentage point; nothing is
converted from tokens, credits or fixture points). The worker launch claims the booking's effects
before its worktree and binds the run in the run's commit. `swarm_reservations` is no longer an
account authority for such a worker: it keeps Swarm's fixture (generic-target) pool policy only.
Evidence: `daemon/tests/shared_launch.rs` (`swarm_worker_admission_books_the_shared_account_and_binds_its_run`),
`daemon/src/swarm/admission.rs`, `docs/verification/swarm/CONTRACT-01.md`. Still partial: no code
produces a qualified per-window upper draw, so the booking inputs are fixture-supplied behind
`OVERSEER_SHARED_BOOKING_FIXTURE_API=1`; Auto's route producer is not yet consumed by Swarm
(targets are still injected snapshots); Gate S's cap replay is not implemented.
