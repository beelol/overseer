# SWARM-06 — quality floor before cost

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`). Support level: daemon fixture policy and Auto's route selection on the proposed native path (`swarm.native_director` switched on inside the test only; the default stays off) with fixture account readings and draws; no provider account, model or VS Code.

Criterion: give a cheap target insufficient capabilities and a qualified target higher cost. Reject the cheap target; if no qualified target exists, block visibly instead of degrading the quality requirement.

Earlier evidence: `policy_uses_capability_allowed_pool_and_finishing_headroom` (`daemon/tests/swarm_policy.rs`, [milestone 4](milestone-4.md)) rejected a cheaper target without the required capability in fixture policy; the route-selection path did not exist yet.

| Clause | Test |
| --- | --- |
| A cheap target lacking capability is rejected for a costlier qualified one | `an_unqualified_cheap_route_is_refused_and_no_qualified_route_blocks_visibly` (`daemon/tests/swarm_native.rs`, new; passed on first run): a job needing the frontier tier is refused the general-tier (cheaper) route with `insufficient_capability` and launched on the frontier route of the same account; `auto_selects_each_jobs_route_within_the_approved_pool` (job b) shows the same; fixture policy: `policy_uses_capability_allowed_pool_and_finishing_headroom` |
| With no qualified target, block visibly; never degrade the requirement | the new test: a job needing a tool no approved route has is `blocked` with `no_eligible_route`, the route decision records every route's reason, no route is admitted, and the job stays `ready`; `malformed_job_capability_requirement_is_rejected_before_dispatch`, `planned_job_capabilities_survive_restart_and_constrain_every_admission_path` |

Run: `cargo test --offline -p overseerd --test swarm_native -- --test-threads=1`, `cargo test --offline -p overseerd --test swarm_policy -- --test-threads=1` (12 passed), 2026-09-28.

Boundary: capability tiers and tools are Auto's recorded priors for the fixture accounts; whether a live model meets a job's quality bar is not measured.
