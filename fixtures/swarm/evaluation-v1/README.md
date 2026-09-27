# Evaluation v1: local serial and parallel comparison

This fixture compares two evidence-producing jobs through Overseer's scripted Swarm worker bridge. It uses `/usr/bin/python3`, disposable local Git worktrees, and a fixture-only `work_units` quota pool. It does not contact a model, provider account, customer backend, or VS Code.

`daemon/tests/swarm_evaluation.rs` fixes four policy cases in `manifest.json`: a dependency chain, independent jobs, conflicting exclusive writes, and a constrained allocation. The independent case also launches two 1.5-second workers once with a one-worker ceiling and once with a two-worker ceiling. Each worker stores an artifact and a result carrying 100 fixture work units. The test accepts both results, applies the director batch, and requires evidence-gated completion in each run. It measures wall time from first admission through completion and reports the difference above the ideal worker delay as local orchestration overhead.

Replay with `cargo test --offline -p overseerd --test swarm_evaluation -- --nocapture`. The test asserts that the parallel run finishes sooner on the local test machine at the same 2/2 acceptance and 200 fixture work units. The values are scripted fixture receipts. Overseer correctly leaves provider `actual_usage_milli` null; this fixture does not prove token savings, native harness usage, or a general speedup.
