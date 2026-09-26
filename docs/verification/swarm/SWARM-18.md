# SWARM-18 — writer isolation and user-work preservation

Status: partial. Revision: `9a55173`. Fixture version: local Git repository and supervised scripted workers 1. No provider account is used.

Input: a committed repository with `a.txt` and a user's uncommitted edit to that file. One category plan has two independent writer jobs. Exact synthetic quota and a beneficial parallel decision admit both. Each job launches a local `/bin/sleep` worker through `swarm.worker.launch`; the test writes a different conflicting `a.txt` body into each worker's assigned workspace.

Expected: the workers use distinct worktrees based on the committed source revision. Each sees only its own edit, and neither launch, edit, nor Stop changes the user's dirty checkout. The jobs stay linked to their respective workspaces.

Observed: both workspace records are `worktree` and have different paths. Each started from committed `a.txt`, then retained its own conflicting edit. A full source checkout fingerprint, including files, status, index, stash and HEAD, matched before launch, after both edits, and after Stop. Both supervised workers received a non-completed terminal state. The focused fixture passed.

Command: `cargo test --offline -p overseerd --test swarm_runtime swarm_writers_keep_conflicting_changes_out_of_a_dirty_source_checkout -- --nocapture` with local process and Unix-socket access.

Evidence: `daemon/tests/swarm_runtime.rs` (`swarm_writers_keep_conflicting_changes_out_of_a_dirty_source_checkout`), `daemon/src/swarm/runtime.rs`, `daemon/src/daemon.rs`. The existing current-checkout one-writer rule is covered separately in `daemon/tests/protocol.rs`.

Follow-up at `14faa93`: accepted patch artifacts apply in an Overseer-owned integration worktree, leaving the source checkout fingerprint unchanged. Two conflicting accepted patches preserve the first integrated commit and the second artifact. An active source-repository commit hook blocks unattended integration before running. The focused integration fixture and full offline Rust suite passed 166 tests.

Follow-up at `fca22c0`: fixture interruptions after patch application and after Git commit recover in the integration worktree after daemon restart, without a duplicate commit or source checkout mutation. Unexpected workspace edits block recovery. The eight focused integration tests and the 168-test offline Rust suite passed.

Remaining: no combined result/check or director conflict resolution was tested. The fixture does not simulate unsaved editor buffers, service-side writes, or all normal launch paths. Keep the RFC box unchecked.
