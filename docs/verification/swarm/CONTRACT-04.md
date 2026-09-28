# CONTRACT-04 — bounded fallback for one job

Status: not started
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
