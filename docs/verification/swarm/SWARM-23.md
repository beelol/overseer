# SWARM-23 — visible Swarm status

Status: partial. Revision: `0997e20`.

Input: a category with one admitted local worker, and a separate category with a planned job but no worker process. Both use the deterministic `OVERSEER_NOTIFY_COMMAND=/usr/bin/true` fixture. The tests call `daemon.background_notice`; the active-worker case is `daemon_stop_all_preserves_unconfirmed_swarm_worker_after_control_loss`, and the queued case is `background_notice_names_queued_swarm_without_a_worker_process` in `daemon/tests/swarm_control.rs`.

Expected: the background notice identifies the category as one Swarm, gives its active worker count, and does not present each worker as an unrelated agent. Queued category work remains visible even with zero worker processes. Ordinary-agent notifications retain their existing wording.

Actual: before the patch, the active worker was announced as one unrelated generic agent and the queued Swarm produced no notice. Both cases now announce one category-level Swarm, with respectively one and zero active workers; the notice payload retains the underlying run IDs and supplies category, status and worker count. The ordinary-agent regression `ac45_last_vscode_window_closing_with_active_runs_posts_a_notice_but_a_reload_does_not` passed. The full offline Rust suite passed 191 tests with 11 ignored using `cargo test --workspace --offline -q`; `git diff --check` passed.

Remaining at the notification revision: the VS Code extension lacked the required Swarm active/queued counts, targets/accounts, limiting constraint, usage state, finishing reserve and decision explanation. A background notification cannot verify those on-screen requirements. This criterion stayed unchecked.

At code revision `e773082`, `swarm.get` also gained per-status job totals and separate registered-attempt and running-supervisor counts; see [SWARM-37](SWARM-37.md). These fields are read APIs, not on-screen evidence, so the remaining UI requirements and this partial status are unchanged.

Sidebar follow-up (this revision): the extension now consumes bounded `swarm.list` summaries and renders active, ready and blocked counts in an expandable Swarms section of the Agents tree. Its unit fixture verifies those rows and lazy filtered job pages; the existing packaged sidebar scenario still passes. The fixture does not establish an on-screen account/usage/limiting-constraint/finishing-reserve readout, and no full Swarm UI scenario has run. SWARM-23 remains partial.
