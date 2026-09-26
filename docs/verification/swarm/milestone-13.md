# Swarm implementation milestone 13 — pre-dispatch resource claims

Fixture admission now validates up to 32 declared read/write resources and acquires their claims in the same SQLite transaction as the attempt and quota reservations. An active writer excludes another writer or reader across categories. A failed admission leaves no attempt; a released claim permits the blocked request to be replayed unchanged.

Evidence: `daemon/tests/swarm_admission.rs`. SWARM-53 remains partial because the runtime does not yet derive claims from actual tool use or detect contamination after dispatch. Claims use exact resource strings and are fixture-supplied; external aliases and live process control are unverified.
