# SWARM-03 — dependency and resource-aware concurrency

Status: partial. Revision: `0d2074c`. Support level: daemon fixture paths only.

Input: a version-1 Swarm plan contains two ready audit jobs that both declare an exclusive `db:shared` write claim. The director supplies paired serial/parallel estimates and marks them independent. A second fixture creates two categories whose ready jobs each declare an exclusive `db:tenant-fixture` write claim, but submits admission requests without any claim. Both use the built-in Swarm policy and synthetic exact quota snapshots. The benefit fixture restarts the daemon after planning.

Expected: a persisted plan claim cannot be omitted or downgraded at admission. Overlapping writers must not be treated as a beneficial parallel batch or receive simultaneous attempts. Planned ownership must remain visible in the job ledger.

Observed: the paired benefit fixture initially chose `parallel`; it now chooses `serial` with `resource_conflict` after restart. The second category's admission initially succeeded; it now returns `resource_conflict` before an attempt is reserved, even though its request omits the planned claim. An attempted write-to-read downgrade is rejected. `swarm.jobs` returns the durable planned claim. A focused replay caught and fixed a regression in idempotent benefit commits after jobs leave `ready`. The full offline Rust suite passed 152 tests (10 unit, 49 protocol, 93 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_benefit -q`; `cargo test --offline -p overseerd --test swarm_admission planned_write_claim_cannot_be_omitted_at_admission -- --nocapture`; `cargo test --offline -p overseerd --test swarm_plan planned_resource_ownership_is_visible_in_job_ledger -q`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_benefit.rs`, `daemon/tests/swarm_admission.rs`, `daemon/tests/swarm_plan.rs`, `daemon/src/swarm/plan.rs`, `daemon/src/swarm/benefit.rs`, `daemon/src/swarm/admission.rs`.

Remaining: the complete serial-chain and three-independent-job replay has not been tied to this evidence record; real worktree write ownership and tool-level scope enforcement remain absent. The model can still fail to declare a needed resource, and the daemon cannot yet discover all actual mutable database/service aliases. The criterion stays unchecked.
