# SWARM-39 — defaults and policy precedence

Status: partial. Revision: `d844816`.

Input: built-in 8-worker/9-global defaults, saved application and category policies, explicit run overrides, and an unconfigured run. A fixture run overrides allocation to 20% and finishing reserve to 30% on a 60,000-point compatible window.

Expected: the run inherits approved targets and normal defaults without per-worker choices. Run overrides win over category and application settings, and a changed preset does not silently rewrite an active run. The effective allocation/reserve values must reach admission, not merely appear in settings.

Actual: precedence and frozen settings were already tested. The new test initially blocked an 8,000-point worker because admission still used hardcoded 10%/20% arithmetic. It now admits that worker, freezes a 12,000-point allocation and 3,600-point reserve, and blocks a further 500-point worker outside the remaining non-finishing headroom. Preview displays applied percentages, and values above 100% are rejected. The workspace suite passed with 75 tests at revision `d844816`.

Evidence: `daemon/tests/swarm_settings.rs`, `daemon/tests/swarm_policy.rs`, `daemon/tests/swarm_admission.rs`, `docs/verification/swarm/milestone-16.md`.

Remaining: no normal S0 start UI or live director/worker launch, one-time account selection, immediate permission revocation or full scheduler integration exists. This remains a fixture-only policy result.
