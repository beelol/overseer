# SWARM-52 — audit-only source and service scope

Status: partial. Implementation revision: `eec9894`. Fixture authority only; no enabled provider-harness path is qualified for read-only execution.

Input: an audit-only run is created without a source-change grant, then a director plan tries to include `source_change_permission: isolated`. A worker supplies a valid patch artifact, the daemon restarts, and the director calls `swarm.integrate`. A separate Catalog S3 run explicitly uses `source_change_permission: isolated` before its 26 accepted patch commits. An invalid `current_checkout` permission is also attempted.

Expected: a plan or worker result cannot add source-write authority. An audit run must not integrate the patch, even after restart; its source checkout remains unchanged. An explicitly authorized implementation run can still integrate into its isolated worktree. Existing runs without a stored grant migrate to `none`.

Actual: before the fix, `audit_only_run_cannot_integrate_a_worker_patch` failed because the audit patch was integrated. At `eec9894`, `swarm.create` persists `none` by default or an explicit `isolated` grant; the run readout and worker brief expose it. Unknown permission values are rejected. `swarm.plan` cannot change the stored grant. After restart, `swarm.integrate` rejects the audit patch before creating an integration worktree, and the source fingerprint is unchanged. Schema migration gives old runs `none`. Explicitly granted integration tests and the Node 24 Catalog replay still pass.

Verification: `cargo test --workspace --offline -q` passed 175 Rust tests (11 unit, 49 protocol, 115 Swarm); `cargo test --offline -p overseerd --test swarm_scenarios -- --ignored` passed the local Node 24 Catalog replay. `git diff --check` passed. Evidence: `daemon/tests/swarm_integration.rs`, `daemon/src/swarm/{mod,schema,integration,context}.rs`, and `daemon/tests/swarm_scenarios.rs`.

Remaining: the generic local worker runs in a writable worktree, so this daemon integration guard cannot prevent that process from editing source before it returns. There is no qualified read-only harness sandbox, live-service mutation boundary, or S1/S2/S4 audit fixture. Worker/source text must also be tested against every future authority-bearing normal launch path. The criterion remains unchecked.
