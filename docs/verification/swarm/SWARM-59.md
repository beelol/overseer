# SWARM-59 — durable eligibility block and wake

Status: partial. Revision: `c0c47c6`. Support level: fixture-only observation of the agreed Auto Mode snapshot shape; no live telemetry feed or user-facing control.

Input: four local replays cover (1) an allowed target missing at category start, daemon restart, repeat observation and later recovery; (2) loss of the only allowed target after the first of two jobs is admitted; (3) insufficient finishing capacity followed by increased headroom; and (4) an otherwise healthy recovery after the original 15-second run deadline. Each observation declares the required capability, native-unit estimate and purpose. The first replay also tries to turn the block into eligibility by changing only the purpose or estimate. A migration fixture supplies a legacy eligible observation without an assessment identity. The mid-run worker submits a discovery and artifact while availability is blocked.

Expected: preserve a specific blocked reason and existing artifacts, stop new admissions and unchanged-state director turns, wake once on an eligible state change, and never wake beyond the original deadline. Active worker reports remain durable.

Observed: the missing target persists as `availability.state=blocked`, `reason=allowed_target_missing` across daemon restart. The repeated observation leaves `wake_count=0` and an empty director batch returns `blocked`. Changing the assessment purpose or estimate is rejected without a wake; a later eligible snapshot for the original assessment records one wake event in the director inbox. The migration marks a legacy eligible row `blocked` with `assessment_unknown`, so the row cannot authorize new work. During mid-run target loss, direct admission and round-robin scheduling hold new work while the first worker's artifact and discovery remain available to the director; restored eligibility admits the next job. Exhausted finishing capacity records `finishing_reserve` and wakes when headroom increases. A recovery observed after the original deadline causes a deadline Stop and no wake. The four focused tests and full offline Rust suite pass (143 tests: 10 unit, 48 protocol, 85 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_availability -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_availability.rs`, `daemon/src/swarm/availability.rs`, `daemon/src/swarm/admission.rs`, `daemon/src/swarm/scheduler.rs`, `daemon/src/swarm/director.rs`.

Remaining: the observation route is fixture-only; Auto Mode does not yet publish live eligibility changes. This is a persisted availability substate, while the top-level run remains `planning`/`running`; a normal status UI is absent. User-triggered target changes and interruption through qualified live provider harnesses are not implemented. A changed assessment, including a migrated legacy assessment with unknown identity, needs an explicit transition or fresh run; the current fixture route fails closed. A live director loop has not been qualified for no repeated model calls. The RFC criterion stays unchecked.

Dispatch S4 follow-up: `daemon/tests/swarm_dispatch_incident.rs` starts a two-job incident run, retains L1's pool-wait artifact, then removes all qualified selected accounts from the fixture snapshot. The availability block persists across daemon restart; repeating the same observation produces no wake, the director remains blocked, and an unselected target cannot resume L2. This is an incident-shaped regression, not a live Auto Mode feed. See [S4](S4.md).

Atlas follow-up at `9a5cf6d`: a joined J2 PostgreSQL task-mutation probe remains reviewable after a fixture observation reduces an allowed account window from 100,000 to 500 milli-points. The availability readout records the later observation time and `finishing_reserve` block; J4 admission is held even when its request carries the earlier larger snapshot. J2's artifact is accepted once, J4 has no admission, and completion is refused with missing coverage. All 14 opt-in Atlas tests pass with disposable PostgreSQL 16 and Node.js 24. This is injected allowance data, not a live shared Auto Mode feed. The readout does not show numeric before/after allowance, so the S5 display requirement and SWARM-59 remain partial.

Observation-fence follow-up: a separate cross-category fixture leaves 1,200 points reserved by one worker while another run observes 1,250 remaining. The latter remains eligible on its own estimate, but admission with an older 100,000-point snapshot now returns `snapshot_superseded`; admission with the observed snapshot returns `shared_pool_headroom`. A newer 3,000-point observation admits the waiting job. The numeric change sets `changed=true` without a false blocked-to-eligible wake; a same-timestamp conflicting numeric observation is refused. Replay: `cargo test --offline -p overseerd --test swarm_admission newer_allowance_observation_fences_stale_admission_snapshot`. This does not yet provide the required live allowance feed or numeric status display.

Revocation follow-up at `656f3f6`: a selected target's injected `auth=revoked` observation now durably marks only its registered job `cancel_requested`, records a director availability event, and requests an interrupt for its linked supervised worker. The other selected account remains eligible. The focused test replays the same observation without duplicate cancellation, restarts the daemon, confirms the reason survives, and leaves the exited attempt's reservation `uncertain`. The joined Atlas replay holds J2 on account B and J4 on account A after both complete real PostgreSQL probes; revoking A interrupts J4 but not J2, rejects a new A admission, and preserves J2's artifact. Five availability tests, seventeen opt-in Atlas tests and the full non-ignored serial daemon suite pass. This targets an explicit revoked target in a fixture snapshot; live account identity feeds, aliases for several targets of one account, and qualified provider interruption remain unverified.

Ordering follow-up (this revision): the targeted revocation now records one durable `revoke` operation in its cancellation transaction, including after an identical observation replay and daemon restart. Three local orderings against result, review, and Stop preserve one result and a readable director inbox. The director-wide availability event intentionally has no job or attempt ID; `swarm.messages` now returns null fields for that event. This remains fixture-only evidence and does not resolve the live-feed or identity-alias gaps.

Account-alias follow-up (this revision): the fixture now treats an explicit `auth=revoked` status as applying to its account ID. Two admitted jobs on different selected routes with the same account ID both receive durable cancellation, even though the second route's target record still says `auth=ok`; an independent account stays eligible and admits the third job. A revoked alias outside the selected pool also blocks the selected alias in the policy preview. The affected local suites pass. Missing target records, verified live account identity, and provider interruption remain open.

Availability closeout at `34c59d52` (2026-09-27): the director may close an otherwise idle
run with a partial report whose reason exactly matches a fresh, durable blocked eligibility
assessment. The request still requires director ownership, plan/control revisions, a bounded
summary and limitations, confirmed worker exits and a drained director inbox. The Stop
transaction saves the report, cancels queued jobs and records an incomplete outcome; a replay
after daemon restart returns the one saved effect. The focused fixture first failed because
`allowed_target_missing` was unsupported, then passed. It also rejects a fabricated
`finishing_reserve` reason, an expired assessment and a migrated assessment missing its
snapshot fingerprint. A migration test preserves an older unresolved-conflict report while
allowing the new reason. `cargo test -p overseerd --test swarm_availability --test
swarm_conflict --test swarm_integration --test swarm_state --offline -- --test-threads=1`
passed 8 + 10 + 25 + 21 tests; the focused migration test and full
`cargo test --workspace --offline -q` passed. Evidence: `daemon/tests/swarm_availability.rs`,
`daemon/src/swarm/mod.rs`, `daemon/src/swarm/schema.rs`. This is an explicit director choice
under a fixture observation; it does not supply live Auto Mode updates, numeric allowance
display, or normal UI, so SWARM-59 stays partial.

Numeric observation follow-up (`491f6cff`, `90cc8619`): a focused daemon replay
records one selected `points` window falling from 100 to 0.5 points, preserving
its previous balance and signed change through restart. Duplicate observations
do not erase that comparison. A later unknown balance stays null and does not
invent a change; 101 observed windows produce a bounded 100-row readout with
an explicit truncated count. The packaged VS Code fixture shows the observed
drop under Capacity and then updates to `unknown` without a job-state change.
See `daemon/tests/swarm_availability.rs` and
`docs/verification/evidence/ui/swarm-allowance/` for checks and screenshots.
The Atlas J2/J4 allowance-drop replay was not rerun with this display, so this
is complementary fixture evidence, not one joined S5 trace. Live Auto updates,
verified account identity, user-triggered target changes and a qualified live
director remain open. SWARM-59 stays partial.
