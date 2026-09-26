# Swarm implementation milestone 15 — late attempt result isolation

The durable broker still records a result from a finished attempt, including output that arrives after a replacement starts. It now changes the logical job to `submitted` only when the reporting attempt is still registered. This prevents a late predecessor result from being mistaken for the replacement's completion.

Evidence: `daemon/tests/swarm_broker.rs`. SWARM-42 remains partial until live dispatch, delivery and accounting races are exercised end to end.
