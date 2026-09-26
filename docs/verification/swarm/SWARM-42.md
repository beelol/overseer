# SWARM-42 — replay and late-message identity

Status: partial. Revision: `67675df`.

Input: one logical job has a first attempt that submits evidence, is rejected and confirms exit. A second attempt starts. The first attempt sends a newly identified late result after the second attempt is reserved.

Expected: the late message is retained for review but cannot submit the replacement attempt or make its result appear complete. The second attempt's own result can submit it.

Actual: the test initially showed the first attempt's late result changing the job to `submitted`. The broker now updates job state only when the reporting attempt is still registered. The late message remains in the director inbox, and the replacement's own result transitions the job. Earlier tests cover stable-ID dedupe and restart replay. The workspace suite passed with 73 tests at revision `67675df`.

Follow-up revision `a0ef330`: `terminal_run_replays_a_saved_result_receipt_without_accepting_new_work` persists a result, simulates run finalization before its receipt reaches the worker, kills and restarts the daemon, then replays the exact message ID and body. Before the fix the terminal-run guard rejected the replay; afterward it returns the original durable sequence as a duplicate. A changed body with the same ID and a new message ID are both rejected. The full offline Rust suite passed 178 non-ignored tests. This is a local broker fixture, not a qualified live harness replay.

Evidence: `daemon/tests/swarm_broker.rs`, `docs/verification/swarm/milestone-15.md`.

Remaining: a live multi-harness reordered message trace, no double dispatch/acceptance/accounting across process recovery, and source-to-director delivery qualification remain unverified.
