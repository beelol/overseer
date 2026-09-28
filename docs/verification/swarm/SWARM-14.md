# SWARM-14 — scoped outage and route failure

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Evidence revision: `fecd933`. Policy snapshot version: 4; availability snapshot version: 1. Local scripted fixtures only; no live provider or account calls.

Input: `rate_limit_and_local_harness_failure_leave_another_provider_eligible` gives two allowed OpenCode provider routes distinct account and pool IDs. The first route independently receives `health=rate_limited`, `local_unavailable`, and `down`, `auth=expired`, or an exhausted exact pool window. The second route stays healthy and funded. `rate_limited_provider_blocks_new_launches_without_stopping_an_independent_peer` starts a three-job run, admits jobs through both routes, then observes a rate limit on route A while B remains healthy.

Expected: the failure excludes only its matching route, identifies its reason, and permits work on the qualified independent route. A rate limit on new A launches does not falsely cancel already admitted B work or erase its evidence.

Actual at the initial revision: preview returns `rate_limited`, `local_harness_unavailable`, `target_unhealthy`, `auth_unavailable`, and a capacity hold for A, while B remains eligible in every case. After the rate-limit observation, the run is still eligible with only B in its eligible set. A new A admission is refused as `rate_limited`; the same ready job admits on B with a distinct request ID. Existing A and B attempts remain reserved rather than being called exited, and B submits a durable discovery. The focused rate-limit test was red with a generic `target_unhealthy` reason before the fix. The affected 51 policy, availability, admission and dispatch tests passed serially.

Scoped-failure follow-up at `67280898`: an allowed OpenCode provider A has two aliases on one declared endpoint, while provider B has a different endpoint and account. A down/rate-limited A observation now excludes both A aliases; B remains eligible and can receive the next admission. An expired login excludes every alias of its account. A declared local OpenCode harness failure excludes both providers because both require that binary. An exhausted A pool returns `quota_exhausted`, distinct from the finishing reserve, while B stays eligible. Unsupported health scopes and endpoint-scoped failures without an endpoint identity are rejected. The focused `failures_follow_declared_account_endpoint_and_harness_scope` test failed before the fix because A's healthy alias remained eligible, then passed. The endpoint-alias admission replay also passed. The scoped policy and availability suites passed 12 and 11 tests respectively. The admission suite passed 39 tests. One dispatch expiry test failed during a concurrent suite run and passed when rerun alone, consistent with the repository's load-sensitive test guidance.

Replay: `cargo test --offline -p overseerd --test swarm_policy --test swarm_availability --test swarm_admission --test swarm_dispatch --quiet -- --test-threads=1`.

Evidence: `daemon/tests/swarm_policy.rs`, `daemon/tests/swarm_availability.rs`, `daemon/src/swarm/policy.rs`, and [CONTRACT-03](CONTRACT-03.md).

Remaining: target health and account quota are injected fixture fields; Auto Mode does not yet feed versioned live scoped failures into shared admission. Model-specific exclusion keys, actual local harness failure and reconnect handling, and provider communication remain unqualified. An active A process under rate limit is not proven reachable or reconciled by this fixture. SWARM-14 and CONTRACT-03 stay unchecked.

## Verified at fixture scope (2026-09-28)

| Clause | Test |
| --- | --- |
| Inject provider outage, account auth failure, quota exhaustion, rate limit and local harness failure independently | `rate_limit_and_local_harness_failure_leave_another_provider_eligible` (`swarm_policy.rs`): route A separately `down`, `auth=expired`, exhausted pool, `rate_limited`, `local_unavailable`, each with its own reason (`target_unhealthy`, `auth_unavailable`, `quota_exhausted`, `rate_limited`, `local_harness_unavailable`) |
| Only affected targets are excluded | `failures_follow_declared_account_endpoint_and_harness_scope`: an endpoint failure excludes both aliases on that endpoint, an expired login every alias of that account, a local harness failure every route needing that binary; an independent provider stays eligible; `revoked_account_blocks_all_of_its_target_aliases`; `one_account_without_a_common_verified_pool_cannot_double_its_allowance` |
| Unrelated healthy jobs continue | `rate_limited_provider_blocks_new_launches_without_stopping_an_independent_peer` (`swarm_availability.rs`: after A is rate limited, B's admitted work keeps running and reports; new work admits on B); `revoked_selected_identity_cancels_only_its_active_attempt_and_keeps_usage_uncertain`; joined `atlas_s5_revoked_account_interrupts_only_affected_backend_worker` |
| OpenCode with a second healthy provider remains eligible if qualified and allowed | the policy tests above use two OpenCode providers: with A failing, B stays eligible and admits |

On Auto's route-selection path the same scoped rules are Auto's (AUTO-AC-09 and AUTO-AC-19, verified at fixture scope in the Auto ledger); Swarm's route selection consumes them and records each exclusion's reason (`each_route_decision_replays_to_the_same_route_and_reason`). OpenCode workers themselves are not yet a Swarm worker harness (excluded before launch as `swarm_worker_launch_unsupported`), so on that path the clause's "if qualified" is not met and nothing is claimed for OpenCode workers.

Rerun serially on 2026-09-28: `swarm_policy` 12, `swarm_availability` 12, the Atlas revocation replay.

Boundary: failures are injected observations; a live outage feed and real harness reconnects are live material.
