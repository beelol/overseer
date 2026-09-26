# CONTRACT-02 — one capacity for one account

Status: not started
Tested implementation commit: none; branches are separate.
Verification date and verifier: 2026-09-26, Swarm implementing agent (contract audit only).
OS / architecture / VS Code / harness versions: pin when integrated test runs.
Harness, provider and redacted account identities: synthetic linked profiles and an independent account; live identity proof not run.
Prerequisites and fixture: Auto account/pool identity reaches the shared admission service without profile-ID substitution.
Steps or exact reproducible commands: route two harness profiles known to share one pool, then an independent profile; exhaust the shared pool and repeat after daemon restart with identity unresolved. Run the integrated daemon test at one pinned revision.
Expected result: linked routes draw one allowance and both block at exhaustion; independent account remains eligible; unresolved identity cannot double apparent capacity.
Actual result: not run across modes. Swarm fixture targets with the same account and ambiguous pool IDs fail closed; Auto's separate branch groups some Codex profiles by authenticated fingerprint but cross-harness identity is unresolved.
Evidence paths: `daemon/tests/swarm_admission.rs`, `docs/verification/swarm/SWARM-09.md` (partial precursor only).
Live vs fixture coverage: fixture precursor; no integrated or live coverage.
Known limitations and remaining platform/account combinations: shared identity proof across harnesses and real subscription pools is absent.
Blocker, attempted alternatives and next action: keep unchecked; integrate identity mapping and run linked, independent and unresolved cases through the same transaction.
