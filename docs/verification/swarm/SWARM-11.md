# SWARM-11 — quota windows and resets

Status: partial. Revision: `3a9d89e`. Fixture version: injected exact quota snapshot 1. No provider account is used.

Input: a two-job category run first sees pool `shared`, window `week-old`, and 1,000 milli-points remaining. Its 10% default allocation is 100 milli-points, with 20 reserved for finishing. The first accepted job consumes a 60-point reservation and confirms exit. A reset snapshot replaces the window with `week-new` and 2,000 milli-points remaining while the original run continues. A second worker requests 70 points, then retries at 20 after the hold.

Expected: a quota reset cannot grant the same run another allocation or forget its earlier reservation. The 70-point request must be held; the 20-point request fits the original 100-point cap and remaining finishing reserve.

Observed: before `3a9d89e`, the new window granted 200 points and admitted the 70-point worker. After the fix, admission retains the smaller prior allocation for the same run/pool/unit and counts each attempt once across old and new window IDs. The 70-point request returns `finishing_reserve`; the 20-point request is admitted with `allocation_milli=100`. The focused test, all 15 admission tests and the full offline Rust workspace suite passed (156 tests: 10 unit, 49 existing protocol, 97 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_admission quota_window_reset_does_not_grant_a_second_run_allocation -- --nocapture`; `cargo test --offline -p overseerd --test swarm_admission -q`; `cargo test --workspace --offline -q`. Daemon integration tests used local process and Unix-socket access.

Evidence: `daemon/tests/swarm_admission.rs` (`quota_window_reset_does_not_grant_a_second_run_allocation`), `daemon/src/swarm/admission.rs`. Existing policy previews for unlike units and multiple windows are in `daemon/tests/swarm_policy.rs`.

Remaining: the reset fixture uses synthetic quota points and unlinked attempts. It does not prove live reset detection, actual native-unit usage reconciliation, unlike-unit end-to-end admission, or a full short/long-window run. Keep the RFC box unchecked.
