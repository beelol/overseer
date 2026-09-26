# SWARM-15 — bounded routing failure retries

Status: partial. Revision: `ca52007`. Support level: fixture-only scripted local workers; the target IDs and availability snapshots are injected, not live Auto Mode routes.

Input: a one-job backend category permits two fixture targets backed by different accounts. The first target admits an attempt whose supervised worker fails to start its missing executable. The daemon observes its terminal state. The same logical job is admitted on the second target, where the same launch failure occurs. Each terminal receipt is replayed. A plan revision then changes the job title. A separate replay records an unknown external effect before its worker fails.

Expected: a confirmed failed launch without a submitted result releases the logical job for one alternate route. A second failure exhausts its two-attempt budget. Neither duplicate receipts nor replanning reset the count. An unknown side effect blocks replacement, including after a failed worker process.

Observed: before the fix, the first failed worker's job remained `reserved`. After the fix, it becomes `ready` with one attempt and then `failed` with two. A replay of either terminal event does not increment the count, and revision leaves the job failed. In the side-effect replay, the failed worker leaves the job `blocked`; admission on the other target is refused. The focused two-test replay and full offline Rust suite pass (138 tests: 9 unit, 48 protocol, 81 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_routing -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_routing.rs`, `daemon/src/swarm/artifacts.rs`, `docs/verification/swarm/SWARM-58.md`.

Remaining: target identities are fixture snapshots, and only a generic supervised process launch was exercised. Live routing and other failure classes remain unqualified. No deterministic proof yet shows that an unchanged blocked/waiting state prevents repeated model-planning calls. The RFC criterion stays unchecked.
