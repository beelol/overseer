# Swarm implementation milestone 12 — evidence revalidation

Artifact acceptance now recomputes the stored content hash as well as checking the source revision and submitted attempt. Duplicate artifact upload checks the existing stored bytes against their recorded hash. A black-box daemon test removes an artifact, changes its source revision and corrupts its content in turn; each state blocks acceptance until the original evidence is restored.

Evidence: `daemon/tests/swarm_plan.rs`. SWARM-54 remains partial because the current artifact store is database-backed and has no live filesystem artifact validation, final-report recheck or access-scoped handoff to a replacement worker.
