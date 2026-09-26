# SWARM-58 — lost acknowledgement of a side-effecting command

Status: partial. Revision: `b9a71f5`. Support level: fixture-only daemon API and local SQLite LedgerPay effect simulation; no live harness or payment provider.

Input: the `evt-42` entitlement-grant attempt records a stable effect/operation identity before execution. A fixture drops the acknowledgement after the intent is committed. The test applies one grant to a separate local LedgerPay database, kills and restarts the daemon, then replays the same intent. It also tries a changed operation under the old effect ID and the same operation under a new effect ID. The director rejects the attempt and confirms exit. A later probe reads the external fixture's counter and submits an `effect_probe` artifact to the director. A second replay revises an active job with an uncertain effect before its original worker exits.

Expected: a lost acknowledgement never grants execution twice. Until the external outcome is reconciled, acceptance and replacement are blocked, while an independent job can continue. A plan revision or changed effect ID cannot silently reset the operation. The probe must be intact and delivered to the director.

Observed: the first `begin` commits then returns an injected error; after restart, replay returns `outcome=unknown`, `may_execute=false`. Changed effect/operation identities are rejected. Unknown effect blocks acceptance and makes the rejected job `blocked` after exit; the independent job stays `ready`. The fixture counter is exactly 1. A non-probe artifact, an undelivered probe and a claim of absence are refused. After a valid submitted probe, the effect records `applied` but still cannot be executed again. Revising either a finished or live attempt leaves the affected job blocked after confirmed exit. The two focused tests and full offline Rust suite pass (135 tests: 9 unit, 48 protocol, 78 Swarm).

Commands: `cargo test --offline -p overseerd --test swarm_effects -- --nocapture`; `cargo test --workspace --offline -q`.

Evidence: `daemon/tests/swarm_effects.rs`, `daemon/src/swarm/effects.rs`, `daemon/src/swarm/schema.rs`, `daemon/src/swarm/artifacts.rs`, `daemon/src/swarm/revision.rs`, `daemon/src/swarm/admission.rs`, and `daemon/src/swarm/broker.rs`.

Remaining: the worker-authored operation identity is only a fixture protocol; actual CLI tool calls are not intercepted or forced through it. The submitted probe artifact is not an independently qualified external-state query, so the daemon conservatively refuses an `absent` outcome and never automatically retries an uncertain effect. S2's signed webhook, queue, two-delivery barrier, protected variant and full director trace are not implemented. Cross-category effect identity, resource retention at Stop, live account/harness support and authoritative outcome adapters remain unverified. The RFC criterion stays unchecked.
