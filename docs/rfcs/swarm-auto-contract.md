# Swarm ↔ Auto Mode integration contract (implementation boundary)

Status: proposed boundary in `codex/swarm-mode`, derived from the separate
`codex/automode-rfc` branch at `c769383`. The other branch's implementation is not merged.
This document makes swarm's expectations concrete without editing automode's files.
The final adapter is contract-tested when its implementation becomes available.

## Responsibilities

Auto Mode discovers/routes eligible harness + provider endpoint + account + model + effort
combinations; obtains health, auth, capability, quota and consumption observations; and
estimates work suitability. Swarm owns category planning, worker count, jobs, attempts,
coordination, reservations, and integration. The daemon's admission transaction is the only
place a target becomes committed work, for swarm and ordinary automatic launches alike.
No second quota collector, per-harness CLI scraper or user routing file is added by swarm.

## Target snapshot

A snapshot has a monotonically increasing version, `observed_ms`, expiry, provenance,
confidence and one or more `Target` records. Each target has stable target/harness/provider
endpoint/account/model/effort identifiers, a list of applicable shared quota-pool IDs,
capabilities (including message delivery/acknowledgement and native-child control), scoped
health/auth status, and eligibility reasons. A pool/window has native unit, fresh remaining
amount or unknown, reset if known, and scope (account, model, endpoint, or other binding
limit). Consumed tokens/usage remain separate observations. A local provider may have an
explicit `not_applicable` subscription quota; this never means infinite machine capacity.

Linked profiles are grouped by verified account/pool identity. If independence cannot be
proved, their capacity cannot be added. A changed identity invalidates prior route eligibility
and reservations remain until their processes are reconciled.

## Request and transaction

`route(job requirements, allowed targets, excluded attempts, snapshot version)` returns an
ordered eligible set and reasons for rejected alternatives. This is advisory: the daemon
validates the selected target and all binding windows again when it atomically records a
reservation, concurrency slot and launch intent. An expired version fails and requests a
fresh snapshot. No model may override a deterministic rejection.

An admitted run has a durable attempt/launch ID and reserved upper estimates in matching
native units. Actual usage reconciles into the same pools. The reservation is retained while
launch, process or descendant termination is uncertain. Pool capacity is shared with ordinary
Overseer runs and other categories. External use can change observed headroom; it cannot be
reserved here and must be disclosed. Multiple windows each bind independently.

This contract does not claim an enforceable hard spend limit when adapters only expose
delayed observations. If no compatible upper estimate or fresh comparable allowance exists,
the default policy declines fan-out while preserving the authorized single-agent path.

## Compatibility and verification

The contract follows the separate Auto Mode RFC's continuous task selection and allowance
model. The current `main` branch has account status and some usage events but no complete
account-scoped quota service; fixture snapshots provide deterministic tests until automode's
implementation can supply them. This dependency is recorded as incomplete, not replaced by
an invented balance. `SWARM-24` remains unchecked until both sides share one actual admission
transaction and contract tests cover route-to-launch changes. Any adjustment to this boundary
requires a versioned change and an explicit compatibility test; it cannot silently change
an active swarm's saved policy or permission set.
