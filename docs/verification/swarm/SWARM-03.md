# SWARM-03 — dependency and resource-aware concurrency

Status: partial. Revision: `0d2074c`. Support level: daemon fixture paths only.

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
