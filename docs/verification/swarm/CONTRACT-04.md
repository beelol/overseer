# CONTRACT-04 — bounded fallback for one job

Status: partial (the Swarm side, fixtures)
Tested implementation commit: none; branches are separate.
Verification date and verifier: 2026-09-26, Swarm implementing agent (contract audit only).
OS / architecture / VS Code / harness versions: pin when integrated test runs.
Harness, provider and redacted account identities: synthetic two-target route set; no live account.
Prerequisites and fixture: shared route request carrying `run_id`, `parent_id`, `job_id`, `attempts_remaining`, scoped exclusions and one durable launch intent.
Steps or exact reproducible commands: fail a target before any effect, route a second target, revise the plan, restart the daemon and attempt a third launch. Repeat with an ambiguous process/tool effect and with an unchanged waiting state.
Expected result: at most two execution attempts for one logical job across all targets/revisions/restart; pre-effect fallback may proceed once; uncertain effects pause without reroute; unchanged state causes no model-planning spin.
Actual result: not run across modes. Swarm fixture routing retains an attempt cap; live Auto dispatch is not connected to its logical-job ledger.
Evidence paths: `daemon/tests/swarm_routing.rs`, `docs/verification/swarm/SWARM-15.md` (partial precursor only).
Live vs fixture coverage: fixture precursor; no integrated or live coverage.
Known limitations and remaining platform/account combinations: upstream-scoped fallback and live process-effect reconciliation unqualified.
Blocker, attempted alternatives and next action: keep unchecked; connect Auto route selection to Swarm attempts and test both known and uncertain effects.

Swarm side at `9c3f6569` (2026-09-28): `daemon/tests/swarm_native.rs`
`one_job_falls_back_once_then_stops_and_an_uncertain_effect_pauses`. Two healthy approved
accounts; the director dispatches through its own token and names no account.
- A worker that fails before any effect returns job x to ready (1 attempt). The next dispatch's
  route, chosen by Auto's selector, is the other account; the failed route is excluded
  `earlier_attempt_failed_on_route` while another is eligible.
- After the second failure x is `failed` with 2 attempts. A third dispatch is refused
  `attempt_limit` before any selection: no route decision is recorded and nothing launches.
- Job y's first attempt records an effect with outcome `unknown`; a dispatch is refused
  `side_effect_unreconciled` with no selection and no launch.
- After a daemon kill and restart, both refusals repeat; x still has 2 attempts, y 1, and the run
  has 3 worker launches in all.

Not yet covered: attempts across plan revisions; an admission refusal after a fitting selection
(the dispatch then tries the next route, bounded by the candidates, but no test forces that race);
the director's own planning turns under an unchanged state; the ordinary-caller half of the common
test; live process-effect reconciliation.
