# SWARM-11 — quota windows and resets

Status: verified for the criterion's deterministic window behavior. Initial revision: `3a9d89e`. Latest evidence revision: `11f40009`. Fixture version: injected exact quota snapshot 1. No provider account is used.

Input: a two-job category run first sees pool `shared`, window `week-old`, and 1,000 milli-points remaining. Its 10% default allocation is 100 milli-points, with 20 reserved for finishing. The first accepted job consumes a 60-point reservation and confirms exit. A reset snapshot replaces the window with `week-new` and 2,000 milli-points remaining while the original run continues. A second worker requests 70 points, then retries at 20 after the hold.

Expected: a quota reset cannot grant the same run another allocation or forget its earlier reservation. The 70-point request must be held; the 20-point request fits the original 100-point cap and remaining finishing reserve.

Observed: before `3a9d89e`, the new window granted 200 points and admitted the 70-point worker. After the fix, admission retains the smaller prior allocation for the same run/pool/unit and counts each attempt once across old and new window IDs. The 70-point request returns `finishing_reserve`; the 20-point request is admitted with `allocation_milli=100`. The focused test, all 15 admission tests and the full offline Rust workspace suite passed (156 tests: 10 unit, 49 existing protocol, 97 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_admission quota_window_reset_does_not_grant_a_second_run_allocation -- --nocapture`; `cargo test --offline -p overseerd --test swarm_admission -q`; `cargo test --workspace --offline -q`. Daemon integration tests used local process and Unix-socket access.

Evidence: `daemon/tests/swarm_admission.rs` (`quota_window_reset_does_not_grant_a_second_run_allocation`), `daemon/src/swarm/admission.rs`. Existing policy previews for unlike units and multiple windows are in `daemon/tests/swarm_policy.rs`.

Support boundary: the reset fixture uses synthetic quota points and unlinked attempts. Live reset detection and actual native-unit usage reconciliation remain open under SWARM-13/24/25; no live account support is claimed here.

Pool-freeze follow-up: the run now persists a cap for every then-approved
comparable pool when its first worker is admitted. On a reset to a new window
ID in the same pool and unit, admission carries the old ceiling forward and
also limits it by the new window's current headroom. The existing
`quota_window_reset_does_not_grant_a_second_run_allocation` regression still
holds the 70-point request and admits 20 points at the 100-point cap. A new
pool selected after work begins cannot use the reset rule to mint an
allocation. This remains an injected-window fixture.

Binding-window qualification at `11f40009`: one admitted job commits separate
reservations for a 100,000-point weekly window, a tighter 5,000-point daily
window and a 20,000-request window. At the default 10% allocation, the saved
caps are 10,000 weekly points, 500 daily points and 2,000 requests; these
amounts remain in their own units and are never added. After a 300-point and
100-request job, another 150-point request is held by the daily finishing
reserve despite ample weekly headroom. Omitting the requests estimate returns
`missing_estimate`, rather than converting points into requests. A 1,700-request
job is held by the request-window reserve even though its 50-point estimate
fits both point windows. A 50-point/100-request job is admitted across all
three windows. `quota_window_reset_does_not_grant_a_second_run_allocation`
continues to prove that a new window ID cannot mint a new task allocation.

Commands: `cargo test -p overseerd --test swarm_admission
short_window_and_unlike_unit_each_bind_admission_without_conversion --offline --
--nocapture`; `cargo test -p overseerd --test swarm_admission --test swarm_policy
--offline --quiet` (38 + 11 passed). Evidence: `daemon/tests/swarm_admission.rs`,
`daemon/tests/swarm_policy.rs`, `daemon/src/swarm/{admission,policy}.rs`.

Ruling: SWARM-11 explicitly requires tests of binding windows, unlike units
and reset behavior; those deterministic daemon tests now cover every clause.
Live provider observations and settlement remain separate integration gaps,
so a live route is not implied by the verified checkbox.
