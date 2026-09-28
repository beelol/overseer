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

Qualified upper draw (`b08646f8`, 2026-09-27): without fixture inputs, Swarm admission of an
account target now books with `BookingDraw::Qualified`: the booking cites the account's latest
reading and current identity and computes the worker's per-window upper draw from at least five
isolated runs of the same harness, model and effort (`daemon/src/upper_draw.rs`), recording its
provenance on the booking intent. Fewer samples refuse `upper_draw_unknown` and report
`draw.samples`/`draw.min_samples`. Evidence: `daemon/tests/shared_launch.rs`
(`qualified_draw_admits_a_swarm_worker_after_five_isolated_runs`: refused at four samples,
admitted at five with the five runs as provenance, the worker launched and bound to that booking).
Still partial: the test's readings are fixture meters (Claude readings written directly, as
Claude has no between-run metadata read), no live delta has been attributed, targets are still
injected snapshots, and Gate S's cap replay is not implemented.

Auto's selector chooses each job's route (`9c3f6569`, 2026-09-28): with Swarm on, the native
director's `swarm_dispatch` no longer names the worker's account, model or effort. It states
requirements only (`min_tier`, `required_tools`, `preferred_harness`, `task_class`); a named
account is refused (`unknown argument (target)`; a generic fixture target can be named only under
the Swarm fixture API). `daemon/src/swarm/route.rs` builds the capability priors of every approved
account profile a Swarm worker can launch on (Claude today; Codex and OpenCode profiles are
excluded `swarm_worker_launch_unsupported`) and applies:
- Auto's eligibility (`auto_select::select`): recorded identity (`unresolved_quota_pool_identity`),
  exhaustion across profiles of one account, scoped health, the endpoint recovery check, tier,
  tools and sandbox;
- fit as the booking computes it: the worker's upper draw (qualified, or a fixture draw behind the
  booking's fixture API) against the account's headroom less its live bookings
  (`estimated_draw_exceeds_allowance`) and against the category's remaining allocation in the
  same windows (`category_allocation_exceeded`). A route the booking could not admit (unknown
  draw, a busy account) is excluded with that reason;
- Auto's task-aware ranking (lowest adequate tier, health, allowance, fit, default, preferred
  harness).
Admission stays the one authority and rechecks everything. Each decision is recorded as a
`swarm_route_decision` event with the selection input, fit evidence and `inference: not_used`.

Evidence: `daemon/tests/swarm_native.rs`
`auto_selects_each_jobs_route_within_the_approved_pool` (pool: two healthy accounts, one at 97%,
one never identified and the director's own unidentified profile; a sixth healthy account outside
the pool). Job a lands on Sonnet/medium of the first eligible account, job b with
`min_tier: frontier` on Opus/high of the same account, job c on the other account's Sonnet because
the first account's category allowance (2,000 left) cannot take the 3,000 draw. The 97% account is
excluded on its own allowance, the unidentified ones on identity, the outside account is never a
candidate, and each worker runs on the profile it was booked on. The four existing native tests
(S0, S0 across a restart, native workers, the four-way launch matrix) pass with the director
naming no account. The whole `swarm_native` file passed 7/7 serially.

Still partial: the draws in these tests are fixture draws on fixture readings (no live account);
route candidates come from recorded identity and readings, not a fresh metadata read at dispatch
(the booking rechecks the generation); only Claude workers exist; Gate S's turn-cap replay with an
approved Swarm director is not implemented.

## Status on 2026-09-28 (`claude/auto-swarm`)

Still partial, now for one reason that fixtures cannot close. Covered at fixture scope this session:

- One admission authority for ordinary, Overseer-started and Swarm work: `ordinary_auto_swarm_and_booked_starts_race_for_the_last_slot_and_one_wins`, `calibrated_auto_units_and_a_swarm_worker_race_for_the_last_window`, `a_second_profile_of_the_same_subscription_races_for_the_same_last_window` (see [SWARM-08](SWARM-08.md)); an agent Overseer starts is refused by the same count (`a_full_house_refuses_overseer_started_agents_but_not_overseers_own_run`, see [SWARM-07](SWARM-07.md)).
- Changed eligibility between routing and launch: the booking rechecks the account generation and the category allocation in its own transaction; route decisions replay from their recorded input (`each_route_decision_replays_to_the_same_route_and_reason`, see [SWARM-04](SWARM-04.md)); a newer observation fences an older admission snapshot (`newer_allowance_observation_fences_stale_admission_snapshot`); an expired snapshot is refused (`startup_does_not_launch_an_admitted_worker_from_an_expired_snapshot`).
- Gate S cannot bypass the transaction: Overseer acts on a swarm only through the swarm's controls (`overseer_controls_a_swarm_only_through_its_controls_and_confirms_what_commits_more`) and cannot steer a worker (`overseer_actions_aimed_at_a_swarm_worker_are_refused_and_offered_to_the_director`).

The gap: "its turns are metered to that allocation". Since the owner's decisions of 2026-09-28 the director's `swarm/director` draw is qualified from its neighbour-bracketed runs at fixture scope and the director books its account (`e7e9e18c`, `a_calibrated_director_books_its_account_with_its_own_draw`), but that booking is an ordinary account booking, not counted against the run's category allocation, so its turns are still not metered to the allocation, with or without Gate S's 100-turn cap. Closing it needs the director's booking to pass the category's remaining allocation (buildable at fixture scope, class (a)) and a live calibration on the owner's personal Claude account (b). The turn-cap replay itself (director continuing while Overseer is capped) is fixture-provable once the metering exists.
