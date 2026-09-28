# CONTRACT-05 — one launch through crash and reconnect

Status: not started
Tested implementation commit: none; branches are separate.
Verification date and verifier: 2026-09-26, Swarm implementing agent (contract audit only).
OS / architecture / VS Code / harness versions: pin when integrated test runs.
Harness, provider and redacted account identities: local synthetic harness; no live account.
Prerequisites and fixture: shared durable launch phases, allowance commitment, workspace ownership and supervised child identity.
Steps or exact reproducible commands: kill the daemon before commitment, after commitment, after external worktree effect, after child-row commit and after process start. Reconnect two clients and replay the same work-unit ID; reconcile confirmed settlement versus uncertain effects.
Expected result: one intent/commitment and one child or a visible uncertain state; no second writer, process or model request; release only after confirmed settlement.
Actual result: not run across modes. Both branches have partial local crash fixtures but no shared transaction. Auto's launch-boundary design explicitly leaves full admission and reconciliation open.
Evidence paths: `daemon/tests/swarm_runtime.rs`, `docs/verification/swarm/SWARM-22.md`, `docs/rfcs/swarm-auto-contract.md` (partial precursors only).
Live vs fixture coverage: fixture precursor; no integrated or live coverage.
Known limitations and remaining platform/account combinations: owner proof for external Git effects, native process settlement and live adapter recovery unqualified.
Blocker, attempted alternatives and next action: keep unchecked; implement shared phases and run the full crash matrix with two clients.
