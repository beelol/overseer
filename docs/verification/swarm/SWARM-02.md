# SWARM-02 — selected accounts and permitted destinations

Status: partial. Reproduce with `cargo test --offline -p overseerd --test swarm_checkpoint -q` and `./fixtures/swarm/dispatch-v1/run-swarm.sh`.

The fixture run saves an allowlist of accounts A and B. After A's worker fails, a newly observed account C remains excluded; the director cannot grant C the checkpoint. B is selected but cannot start the replacement until it has a destination-specific artifact grant. The joined S4 replay separately rejects an unqualified cheap target. This verifies selected-target and checkpoint-destination checks in the daemon fixture, including a denied candidate during recovery.

Live account discovery, actual provider/context permissions, a denied-only routing replay and ordinary fallback are not yet exercised. Keep the RFC box open.
