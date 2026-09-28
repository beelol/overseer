# SWARM-03 — dependency and resource-aware concurrency

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. First revision: `2c9b1c5`.

Input: a version-1 Swarm plan contains two ready audit jobs that both declare an exclusive `db:shared` write claim. The director supplies paired serial/parallel estimates and marks them independent. A second fixture creates two categories whose ready jobs each declare an exclusive `db:tenant-fixture` write claim, but submits admission requests without any claim. Both use the built-in Swarm policy and synthetic exact quota snapshots. The benefit fixture restarts the daemon after planning.

Expected: a persisted plan claim cannot be omitted or downgraded at admission. Overlapping writers must not be treated as a beneficial parallel batch or receive simultaneous attempts. Planned ownership must remain visible in the job ledger.

Observed: the paired benefit fixture initially chose `parallel`; it now chooses `serial` with `resource_conflict` after restart. The second category's admission initially succeeded; it now returns `resource_conflict` before an attempt is reserved, even though its request omits the planned claim. An attempted write-to-read downgrade is rejected. `swarm.jobs` returns the durable planned claim. A focused replay caught and fixed a regression in idempotent benefit commits after jobs leave `ready`. The full offline Rust suite passed 152 tests (10 unit, 49 protocol, 93 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_benefit -q`; `cargo test --offline -p overseerd --test swarm_admission planned_write_claim_cannot_be_omitted_at_admission -- --nocapture`; `cargo test --offline -p overseerd --test swarm_plan planned_resource_ownership_is_visible_in_job_ledger -q`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_benefit.rs`, `daemon/tests/swarm_admission.rs`, `daemon/tests/swarm_plan.rs`, `daemon/src/swarm/plan.rs`, `daemon/src/swarm/benefit.rs`, `daemon/src/swarm/admission.rs`.

Remaining: the complete serial-chain and three-independent-job replay has not been tied to this evidence record; real worktree write ownership and tool-level scope enforcement remain absent. The model can still fail to declare a needed resource, and the daemon cannot yet discover all actual mutable database/service aliases. The criterion stays unchecked.

Follow-up at `be63985`: the joined Atlas S5 malformed-plan replay rejects a J8/J9 cycle and missing J99 dependency before any attempt can register. Explicit partial-plan permission retains an independent J2 audit through real backend evidence and completion. `./fixtures/swarm/atlas-v1/run-swarm.sh` passes nine joined tests. This checks graph isolation but does not close the serial-chain, three-independent-job, or live resource-enforcement gaps above.

Follow-up at `53df06a`: `dependency_chain_explains_serial_work_while_independent_jobs_admit`
replays a root→child→leaf chain, three independent jobs, and two jobs with the
same exclusive database claim. Before the fix, a dependent admission returned
only `job_not_ready`. It now returns `dependency_pending` and the precise
`waiting_on` job IDs, using the same satisfaction check that requires accepted
evidence, confirmed exit and integrated patches. The three independent jobs
and root receive separate admissions in one benefit-approved wave; a later
writer receives an admission and the conflicting writer is held without an
attempt. A second serial run accepts and confirms each predecessor before its
successor becomes ready. An integration fixture shows that an accepted but
unintegrated contract patch is still named as the consumer's blocker. The
focused red/green replay passed, followed by the 34 admission, 19 integration,
3 evaluation, 9 benefit, 10 plan and 4 scheduler tests. One pre-existing
three-slot test exposed a race with daemon-owned automatic exit confirmation;
its assertion now checks the stored attempt and process receipt before
accepting a released slot. `git diff --check` passed.

This is a deterministic fixture replay of admissions and accepted evidence,
not a supervised simultaneous execution of these exact jobs. Actual
undeclared-resource discovery, tool-level worktree writer enforcement and a
joined live path remain open. SWARM-03 stays partial.

Follow-up at `3a21e93`: a real daemon and five supervised `/bin/sleep` workers
replay the root, three independent jobs and first database writer against the
same plan claims. The first four start in the default growth wave; an early
fifth admission returns `growth_wave_full`, then the next wave admits it.
All five run concurrently as distinct daemon-tracked processes. While the
first exclusive database writer remains active, a second writer receives
`resource_conflict` and no sixth attempt. Stop interrupts and confirms all
five worker exits, leaving no fixture process running. Expected and observed
statuses match. Reproduce with `cargo test -p overseerd --test swarm_runtime
independent_jobs_execute_together_while_conflicting_writer_waits --offline`;
the affected admission (34), benefit (9) and runtime (23) suites pass together.
At this revision, the earlier root→child→leaf fixture still proves serial
dependency ordering without supervised processes. The follow-up below adds
that process-level replay.

Follow-up at `2c9b1c5`: each root→child→leaf job launches a daemon-supervised
worker that waits for a fixture release file. A dependent admission names its
parent before result acceptance and still names it after acceptance while the
process is live. The fixture then releases the parent, observes its completed
process receipt and confirms exit; only then is the dependent admitted. Each
worker supplies a distinct artifact, result and director acceptance, and all
three jobs finish accepted. Reproduce with `cargo test -p overseerd --test
swarm_runtime supervised_dependency_chain_waits_for_accepted_result_and_exit
--offline`. The complete runtime suite passes 24 tests. The two supervised
replays cover declared dependency and resource coordination without touching
the owner's editor or a live provider. Real worktree writer enforcement,
undeclared resource discovery and a qualified live route leave SWARM-03
partial.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| Replay a serial dependency chain, three independent jobs and overlapping write ownership | `dependency_chain_explains_serial_work_while_independent_jobs_admit` (one plan with root→child→leaf, three independent jobs and two writers of `db:shared`; a second run walks the chain in order) |
| Only ready independent jobs run concurrently | the same test (the root, the three independent jobs and one writer are admitted in one wave; five attempts, none for the held jobs); supervised: `independent_jobs_execute_together_while_conflicting_writer_waits` (five real processes run together across two growth waves while the conflicting writer is held without an attempt), `supervised_dependency_chain_waits_for_accepted_result_and_exit` |
| Serial execution has an explicit reason | `dependency_pending` with `waiting_on` naming the blocking job; `resource_conflict` for the second writer; a benefit decision of `serial` with `resource_conflict` when overlapping writers are proposed as a batch (`planned_exclusive_claim_overrides_director_parallel_estimate`); `growth_wave_full` for a wave already at its size |

Rerun serially on 2026-09-28: `swarm_admission` 39, `swarm_runtime` 25, `swarm_benefit` 9.

Boundary: resource ownership is what the plan declares; discovering undeclared resources and tool-level writer enforcement inside a live harness are not claimed.
