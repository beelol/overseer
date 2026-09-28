# SWARM-52 — audit-only source and service scope

Status: partial. Implementation revisions: `eec9894`; native Claude audit workers `0f38f758` (2026-09-28). Fixture authority only; Claude's read-only audit path is built and checked on the synthetic fixture, not live.

Input: an audit-only run is created without a source-change grant, then a director plan tries to include `source_change_permission: isolated`. A worker supplies a valid patch artifact, the daemon restarts, and the director calls `swarm.integrate`. A separate Catalog S3 run explicitly uses `source_change_permission: isolated` before its 26 accepted patch commits. An invalid `current_checkout` permission is also attempted.

Expected: a plan or worker result cannot add source-write authority. An audit run must not integrate the patch, even after restart; its source checkout remains unchanged. An explicitly authorized implementation run can still integrate into its isolated worktree. Existing runs without a stored grant migrate to `none`.

Actual: before the fix, `audit_only_run_cannot_integrate_a_worker_patch` failed because the audit patch was integrated. At `eec9894`, `swarm.create` persists `none` by default or an explicit `isolated` grant; the run readout and worker brief expose it. Unknown permission values are rejected. `swarm.plan` cannot change the stored grant. After restart, `swarm.integrate` rejects the audit patch before creating an integration worktree, and the source fingerprint is unchanged. Schema migration gives old runs `none`. Explicitly granted integration tests and the Node 24 Catalog replay still pass.

Verification: `cargo test --workspace --offline -q` passed 175 Rust tests (11 unit, 49 protocol, 115 Swarm); `cargo test --offline -p overseerd --test swarm_scenarios -- --ignored` passed the local Node 24 Catalog replay. `git diff --check` passed. Evidence: `daemon/tests/swarm_integration.rs`, `daemon/src/swarm/{mod,schema,integration,context}.rs`, and `daemon/tests/swarm_scenarios.rs`.

Remaining: the generic local worker runs in a writable worktree, so this daemon integration guard cannot prevent that process from editing source before it returns. There is no qualified read-only harness sandbox, live-service mutation boundary, or S1/S2/S4 audit fixture. Worker/source text must also be tested against every future authority-bearing normal launch path. The criterion remains unchecked.

Audit admission follow-up (`8d9f328`): a local socket fixture first showed an
audit-only run admitting a Claude-labeled worker and reserving quota with no
qualified source-write boundary. Admission now returns
`audit_source_boundary_unqualified` before an attempt or reservation is
recorded for every non-fixture target in an audit-only run. An explicitly
granted isolated-write run remains admissible. A second red fixture simulated
a pending native admission from before this gate and showed that it could
create a worker launch record; process launch now rechecks the stored run
permission and refuses that stale reservation before writing a launch intent.
The fixture-only generic process path remains available for deterministic
audit replays.

Checks: `cargo test -p overseerd --test swarm_admission --test swarm_runtime
--test swarm_dispatch --test swarm_integration` passed 33 + 22 + 6 + 19
local tests. The focused admission fixture passed again after extending the
gate to director-executed work. `git diff --check` passed. No paid or live
provider was used. The native Claude fixture tests now declare the explicit
isolated-write grant because they test route binding and descendant ordering,
not audit-only execution.

This prevents an unsupported audit-only native start; it does not qualify any
native read-only path. Generic fixture processes can still edit their isolated
worktrees, and source/service mutation enforcement, real read-only harness
sandboxing, joined S1/S2/S4 audit replay and normal-launch permissions remain
open. SWARM-52 stays partial.

## Native Claude workers in audits (2026-09-28, `0f38f758`)

The owner's decision 3: a native Claude worker may take part in an audit in Claude's read-only permission mode with writes denied, and the daemon checks after the attempt that no source file changed; any change fails the attempt and is reported.

- **Enforcement recorded per harness.** Claude: `--permission-mode plan` on every turn (a turn asking for another mode is refused) and `--disallowedTools Agent,Task,Edit,Write,MultiEdit,NotebookEdit,Bash`, then the daemon's post-attempt check. Generic fixture processes: isolated test inputs, unchanged. Codex, the Codex app-server and OpenCode: no controlled Swarm worker path (`uncontrolled_native_delegation`), and the audit gate would refuse them next (`audit_source_boundary_unqualified`).
- **The check** (`daemon/src/swarm/audit.rs`): HEAD against the pinned revision, `git status --untracked-files=all` (ignored files are not source), and every tracked file's content hash against the pinned tree, so `skip-worktree`/`assume-unchanged` cannot hide an edit. It runs once, after the worker's process and native descendants are gone, and is recorded in `swarm_audit_checks`. A check that cannot be made fails closed.
- **A failed check:** the director cannot accept the attempt (accepting waits for the check), the job is blocked `audit_source_changed`, the director's terminal message carries the check, an event `swarm_audit_source_changed` on the worker's run appears in Overseer's digest of it, and the worktree plus `refs/overseer/audit/<attempt>` keep the evidence.

Tests (`--test-threads=1`): protocol `native_claude_workers_audit_read_only_and_a_source_change_fails_the_attempt` (`daemon/tests/swarm_native.rs`; red before the change: `no_eligible_route`, every Claude route refused `audit_source_boundary_unqualified`). On the product path with the synthetic Claude director and workers, two audit jobs go to Claude workers in `plan` mode with the seven tools denied; worker `a` reads only, its check is clean, and its acceptance is recorded after the check; worker `b` writes `src/roles.txt` directly (as if past its mode), its check lists the file, the director's accept is refused `audit_source_changed`, the job is `blocked`/`audit_source_changed`, the terminal message and Overseer's digest report it, the worktree keeps the file and the evidence ref shows its content. Unit `swarm::audit::tests::the_source_check_finds_every_kind_of_change_against_the_pinned_revision` (clean; ignored output; changed, added, deleted, committed and `skip-worktree`-hidden changes), `only_claude_has_a_qualified_audit_boundary`, `adapters::…::a_read_only_swarm_worker_has_its_write_tools_denied`. `swarm_admission::audit_admits_claude_read_only_and_holds_every_other_native_worker` and `swarm_runtime::previously_admitted_native_worker_launches_read_only_after_audit_scope_is_restored` replace the two tests that asserted the old refusal for Claude.

Still open: the check sees the worker's workspace only, not writes to another checkout, the machine or a live service (no live-service mutation boundary exists); no live Claude has run in `plan` mode under Swarm, so that its read-only mode holds for a real model is unverified; S1/S2/S4 have not been replayed with native audit workers. SWARM-52 stays partial.
