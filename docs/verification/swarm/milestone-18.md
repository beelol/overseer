# Milestone 18 — accepted patch integration foundation

Revision: `14faa93`. This is partial SWARM-18/19/35 evidence, not acceptance-criterion completion.

The focused integration test was red for the absent method, stale source acceptance, premature dependent readiness, premature final completion, and source Git hook execution. The implemented fixture path applies reviewed patches in an Overseer-owned worktree, records each integration commit, keeps user checkout contents and Git state unchanged, and fails closed on source changes, patch overlap and active commit hooks. Six local integration fixtures passed. `cargo test --workspace --offline -q` passed 166 tests; `git diff --check` passed.

Ruling: keep the integration API behind fixture opt-in until normal director/adapter authority is qualified — live user work cannot be integrated through an unverified control path; the cost is that ordinary Swarm runs still cannot use this method.

Ruling: reject active source Git hooks rather than silently skipping or executing them during unattended integration — this prevents an unreviewed hook side effect; the cost is blocked integration for repositories with active hooks until a reviewed flow exists.

Next: add crash recovery between Git commit and SQLite acknowledgement, combined checks on the integration tree, and a director decision for conflicts. No RFC checkbox changes at this milestone.
