# SWARM-18 — writer isolation and user-work preservation

Status: partial. Revision: `9a55173`. Fixture version: local Git repository and supervised scripted workers 1. No provider account is used.

Input: a committed repository with `a.txt` and a user's uncommitted edit to that file. One category plan has two independent writer jobs. Exact synthetic quota and a beneficial parallel decision admit both. Each job launches a local `/bin/sleep` worker through `swarm.worker.launch`; the test writes a different conflicting `a.txt` body into each worker's assigned workspace.

Expected: the workers use distinct worktrees based on the committed source revision. Each sees only its own edit, and neither launch, edit, nor Stop changes the user's dirty checkout. The jobs stay linked to their respective workspaces.

Observed: both workspace records are `worktree` and have different paths. Each started from committed `a.txt`, then retained its own conflicting edit. A full source checkout fingerprint, including files, status, index, stash and HEAD, matched before launch, after both edits, and after Stop. Both supervised workers received a non-completed terminal state. The focused fixture passed.

Command: `cargo test --offline -p overseerd --test swarm_runtime swarm_writers_keep_conflicting_changes_out_of_a_dirty_source_checkout -- --nocapture` with local process and Unix-socket access.

Evidence: `daemon/tests/swarm_runtime.rs` (`swarm_writers_keep_conflicting_changes_out_of_a_dirty_source_checkout`), `daemon/src/swarm/runtime.rs`, `daemon/src/daemon.rs`. The existing current-checkout one-writer rule is covered separately in `daemon/tests/protocol.rs`.

Remaining: this demonstrates Swarm launch isolation and source preservation, not safe integration of conflicting patches. No worker submitted either patch for director review, and no combined result or conflict resolution was tested. The fixture does not simulate unsaved editor buffers, service-side writes, or all normal launch paths. Keep the RFC box unchecked.
