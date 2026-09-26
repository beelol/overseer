# SWARM-05 — serial versus parallel benefit

Status: partial. Revision: `297d5c9`. Support level: fixture-only, read-only benefit preview.

Input: two paired plans estimate the same two independent jobs. Each plan supplies planning, context transfer, worker execution, retry allowance, integration and review costs in elapsed milliseconds and native quota units. The fixture compares a 240 ms serial plan with a 210 ms parallel plan, 40 milli-points of allocation and 10 milli-points reserved to finish. Variants increase parallel context cost to remove the time benefit, reduce finishing reserve, exceed total allocation, make the jobs dependent, or make both plans unaffordable. A second fixture removes a phase estimate, changes a job identity, or supplies an uncalibrated worker estimate.

Expected: choose parallel only for independent jobs with a strict time benefit, affordable total usage and enough finishing capacity. Prefer serial on ties. Reject incomplete or inconsistent estimates.

Observed: the read-only `swarm.benefit.preview` selects the two-worker parallel plan with a recorded 30 ms expected gain, 35 milli-points total usage and 10 milli-points finishing cost. It returns serial when coordination erases the benefit, reserve or allocation is insufficient, or jobs are dependent; it reports blocked when neither path is affordable. Malformed estimates fail. The two focused tests and full offline Rust suite pass (145 tests: 10 unit, 48 protocol, 87 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_benefit -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_benefit.rs`, `daemon/src/swarm/benefit.rs`.

Remaining: a real admission path does not require or persist the preview decision. Estimates and actual elapsed/usage outcomes are not retained for calibration, and the estimator trusts caller-supplied costs and independence. Auto Mode must supply fresh native-unit headroom and the shared reservation authority before live scheduling uses this decision. The RFC criterion stays unchecked.
