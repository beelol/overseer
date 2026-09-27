# SWARM-15 — bounded routing failure retries

Status: partial. Revision: `ca52007`. Support level: fixture-only scripted local workers; the target IDs and availability snapshots are injected, not live Auto Mode routes.

Input: a one-job backend category permits two fixture targets backed by different accounts. The first target admits an attempt whose supervised worker fails to start its missing executable. The daemon observes its terminal state. The same logical job is admitted on the second target, where the same launch failure occurs. Each terminal receipt is replayed. A plan revision then changes the job title. A separate replay records an unknown external effect before its worker fails.

Expected: a confirmed failed launch without a submitted result releases the logical job for one alternate route. A second failure exhausts its two-attempt budget. Neither duplicate receipts nor replanning reset the count. An unknown side effect blocks replacement, including after a failed worker process.

Observed: before the fix, the first failed worker's job remained `reserved`. After the fix, it becomes `ready` with one attempt and then `failed` with two. A replay of either terminal event does not increment the count, and revision leaves the job failed. In the side-effect replay, the failed worker leaves the job `blocked`; admission on the other target is refused. The focused two-test replay and full offline Rust suite pass (138 tests: 9 unit, 48 protocol, 81 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_routing -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_routing.rs`, `daemon/src/swarm/artifacts.rs`, `docs/verification/swarm/SWARM-58.md`.

Remaining: target identities are fixture snapshots, and only a generic supervised process launch was exercised. Live routing and other failure classes remain unqualified. No deterministic proof yet shows that an unchanged blocked/waiting state prevents repeated model-planning calls. The RFC criterion stays unchecked.

Gate L reconciliation at `78befc0`: Continuity's ordinary successor handoff and
retry lack Swarm job/attempt authority. A linked worker or director now refuses
`run.targets` and `run.handoff`, and a linked failed process is not parked for
Continuity's 36-hour retry. The director must perform any later attempt through
Swarm admission. The fixture tests in `swarm_runtime.rs` and
`swarm_director_process.rs` prove the command boundary; they do not prove a
joined live provider outage or same-job failover. See
`main-reconciliation-2026-09-27.md`.

Cross-check at `884ac97` (2026-09-27):
`daemon/tests/swarm_integration.rs::exhausted_integrated_patch_stays_incomplete_after_late_conflict`
uses two real attempts for a dependent patch: rejected evidence, then an
accepted integrated patch. A late evidence conflict and plan revision leave
the same logical job failed at count two with `attempts_exhausted` in the
coverage readout; a third attempt is refused. Independent reproduction can
resolve the route disagreement, but it does not restore attempt budget or
qualify the stale integrated branch. This extends fixture evidence for the
cross-revision cap. Shared Auto Mode routing and unchanged waiting-state
model wakeups remain unverified, so SWARM-15 stays partial.
