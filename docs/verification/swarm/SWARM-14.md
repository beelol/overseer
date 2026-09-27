# SWARM-14 — scoped outage and route failure

Status: partial. Evidence revision: `fecd933`. Policy snapshot version: 4; availability snapshot version: 1. Local scripted fixtures only; no live provider or account calls.

Input: `rate_limit_and_local_harness_failure_leave_another_provider_eligible` gives two allowed OpenCode provider routes distinct account and pool IDs. The first route independently receives `health=rate_limited`, `local_unavailable`, and `down`, `auth=expired`, or an exhausted exact pool window. The second route stays healthy and funded. `rate_limited_provider_blocks_new_launches_without_stopping_an_independent_peer` starts a three-job run, admits jobs through both routes, then observes a rate limit on route A while B remains healthy.

Expected: the failure excludes only its matching route, identifies its reason, and permits work on the qualified independent route. A rate limit on new A launches does not falsely cancel already admitted B work or erase its evidence.

Actual: preview returns `rate_limited`, `local_harness_unavailable`, `target_unhealthy`, `auth_unavailable`, and `finishing_reserve` respectively for A, while B remains eligible in every case. After the rate-limit observation, the run is still eligible with only B in its eligible set. A new A admission is refused as `rate_limited`; the same ready job admits on B with a distinct request ID. Existing A and B attempts remain reserved rather than being called exited, and B submits a durable discovery. The focused rate-limit test was red with a generic `target_unhealthy` reason before the fix. The affected 51 policy, availability, admission and dispatch tests pass serially.

Replay: `cargo test --offline -p overseerd --test swarm_policy --test swarm_availability --test swarm_admission --test swarm_dispatch --quiet -- --test-threads=1`.

Evidence: `daemon/tests/swarm_policy.rs`, `daemon/tests/swarm_availability.rs`, `daemon/src/swarm/policy.rs`, and [CONTRACT-03](CONTRACT-03.md).

Remaining: target health and account quota are injected fixture fields; Auto Mode does not yet feed versioned live scoped failures into shared admission. Endpoint/model-specific exclusion keys, actual local harness failure and reconnect handling, and provider communication remain unqualified. An active A process under rate limit is not proven reachable or reconciled by this fixture. SWARM-14 and CONTRACT-03 stay unchecked.
