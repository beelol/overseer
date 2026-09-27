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

## Read-only integration check — 2026-09-27

Inputs: Swarm `2927b431` and the locally active Auto branch `68dea8f4`;
published `origin/main` remains `b84f9984` and is already in Swarm. Commands:
`git merge-base HEAD codex/automode-rfc`, `git merge-tree <base> HEAD codex/automode-rfc`,
and a count of conflict markers by merged file. The dry merge has 25 conflict
hunks in seven files: `adapters.rs` (2), `daemon.rs` (9), `git.rs` (3),
`server.rs` (3), `store.rs` (6), the daemon test helper (1), and the
extension entry point (1). The branches also overlap without a textual
conflict in six more files. This is a compatibility forecast, not an
integrated build or a merge into either active branch.

The normal Swarm path still cannot satisfy this contract: `swarm.create` is
public, but native director launch, planning, admission and dispatch remain
fixture-gated, and the scripted director launch does not make a shared account
allowance commitment. Auto's in-flight collector and route code must be
consumed at the shared transaction; copying its collector or simply removing
the fixture gate would give a false S0 launch. Resolve the overlapping daemon
entry points when Auto's integration surface is stable, then make the first
joined test race an ordinary start, Auto child and Swarm worker against one
binding short and long window and one writer. No CONTRACT-01 or SWARM-01 box
is checked by this audit.
