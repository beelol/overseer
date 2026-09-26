# SWARM-49 — bounded inbox and admission backpressure

Status: partial. Revision: `042b3e5`.

Input: one planned run with two ready jobs, a registered first attempt and 1,000 distinct progress reports. A second admission uses a fresh, compatible fixture quota snapshot. The active attempt then submits a terminal result.

Expected: routine progress is capped and deduplicated before director inference; new workers wait at the inbox threshold, but terminal events from in-flight workers remain durable.

Actual: `swarm.admit` returns `director_inbox_full` for the second job, and the first job's terminal result changes it to `submitted`. The existing broker test also rejects a 1,001st routine message and accepts a terminal result at capacity. The focused test initially failed because the second admission succeeded; it passed after the admission gate was added. The workspace suite passed with 67 tests at revision `042b3e5`.

Evidence: `daemon/tests/swarm_admission.rs`, `daemon/tests/swarm_broker.rs`, `docs/verification/swarm/milestone-9.md`.

Remaining: malformed permission cases, live director inference/context limits, and measured Stop responsiveness are unverified.
