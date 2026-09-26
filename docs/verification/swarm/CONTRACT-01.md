# CONTRACT-01 — shared atomic admission

Status: not started
Tested implementation commit: none; the Auto and Swarm branches are separate.
Verification date and verifier: 2026-09-26, Swarm implementing agent (contract audit only).
OS / architecture / VS Code / harness versions: not applicable to the proposed contract; pin versions when the integrated test runs.
Harness, provider and redacted account identities: synthetic shared pool and two synthetic clients; live identities not yet exercised.
Prerequisites and fixture: one daemon-owned admission service used by ordinary, Auto and Swarm launch paths, with a known binding short/long account window and one workspace writer.
Steps or exact reproducible commands: race two different callers for the last allowance and writer; change account generation after route selection; replay the winning work-unit ID from two clients. Run the integrated daemon test and `cargo test --workspace --offline -q -- --test-threads=1` at one pinned revision.
Expected result: one commitment, slot, writer and durable intent; the loser waits or pauses with a specific reason; changed identity requires revalidation; replay cannot reserve twice.
Actual result: not run. Swarm fixture admission revalidates injected snapshots and shares an app agent-slot lock with ordinary starts, but account allowance is not shared with Auto or ordinary starts.
Evidence paths: `daemon/tests/swarm_admission.rs`, `docs/rfcs/swarm-auto-contract.md` (partial precursor only).
Live vs fixture coverage: fixture precursor; no integrated or live coverage.
Known limitations and remaining platform/account combinations: Auto producer and atomic cross-mode allowance commitment are absent on this branch.
Blocker, attempted alternatives and next action: keep unchecked; integrate one reservation authority with Auto and run the concurrent multi-window test.
