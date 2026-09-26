# SWARM-46 — changed shared contract and withdrawn conclusions

Status: partial. Fixture: `catalog-v1` version 1, Node.js 24 and scripted daemon APIs. Input: the S3 contract job first submits a timestamp-only cursor; an `accounts` worker submits a patch result against revision 1. The director revises the contract to `(createdAt,id)` and retains the same contract job identity for attempt 2.

Expected: the in-flight worker receives a revision-2 redirect, its old result cannot be accepted or integrated, and dependent work waits for the repaired contract. A separate S1 retraction must later propagate a corrected discovery to all prior recipients.

Actual: `cargo test --offline -p overseerd --test swarm_scenarios -- --ignored` observed one persisted redirect, rejected acceptance of the stale `accounts` artifact, and kept the module dependent on the repaired contract. The contract repair used attempt 2 of the original job. The local Catalog backend replay independently showed that timestamp-only pagination loses a tied row while `(createdAt,id)` passes. See [S3](S3.md) for the full fixture and command.

Remaining: the scripted worker did not acknowledge applying the redirect inside a live model tool call. The scenario did not preserve an unrelated accepted job, retract S1's D1 conclusion, or prove cross-harness delivery. This criterion remains unchecked.
