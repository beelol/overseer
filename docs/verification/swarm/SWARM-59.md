# SWARM-59 — durable eligibility block and wake

Status: partial. Revision: `421628a`. Support level: fixture-only observation of the agreed Auto Mode snapshot shape; no live telemetry feed or user-facing control.

Input: four local replays cover (1) an allowed target missing at category start, daemon restart, repeat observation and later recovery; (2) loss of the only allowed target after the first of two jobs is admitted; (3) insufficient finishing capacity followed by increased headroom; and (4) an otherwise healthy recovery after the original 15-second run deadline. Each observation declares the required capability, native-unit estimate and purpose. The mid-run worker submits a discovery and artifact while availability is blocked.

Expected: preserve a specific blocked reason and existing artifacts, stop new admissions and unchanged-state director turns, wake once on an eligible state change, and never wake beyond the original deadline. Active worker reports remain durable.

Observed: the missing target persists as `availability.state=blocked`, `reason=allowed_target_missing` across daemon restart. The repeated observation leaves `wake_count=0` and an empty director batch returns `blocked`. A later eligible snapshot records one wake event in the director inbox. During mid-run target loss, direct admission and round-robin scheduling hold new work while the first worker's artifact and discovery remain available to the director; restored eligibility admits the next job. Exhausted finishing capacity records `finishing_reserve` and wakes when headroom increases. A recovery observed after the original deadline causes a deadline Stop and no wake. The four focused tests and full offline Rust suite pass (142 tests: 9 unit, 48 protocol, 85 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_availability -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_availability.rs`, `daemon/src/swarm/availability.rs`, `daemon/src/swarm/admission.rs`, `daemon/src/swarm/scheduler.rs`, `daemon/src/swarm/director.rs`.

Remaining: the observation route is fixture-only; Auto Mode does not yet publish live eligibility changes. This is a persisted availability substate, while the top-level run remains `planning`/`running`; a normal status UI is absent. User-triggered target changes and interruption of affected live workers are not implemented. A live director loop has not been qualified for no repeated model calls. The RFC criterion stays unchecked.
