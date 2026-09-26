# Dispatch S4 local incident fixture

This versioned Go/PostgreSQL service fixture creates a two-delivery queue table in
a disposable schema. `consumeWithRetry` holds a transaction, and a one-connection
pool makes a concurrent shipment query time out while waiting for that connection.
The exported JSON records a sanitized trace ID, observed pool wait, observed idle
transaction, the retry handler source reference, and the queue redelivery path.
The 10:00–10:15 timestamps are fictional incident labels; the pool wait and
transaction count come from the local run. The fixture does not access a live
shipment service.

`./run-local.sh` runs the Go acceptance test and prints a sample exported bundle
against disposable PostgreSQL 16. `./run-swarm.sh` runs the opt-in joined Swarm
replay against a separate disposable container. Both runners bind PostgreSQL to
a random localhost port and remove the container afterward. The Go module uses
cached `pgx` dependencies for offline runs.

The joined replay scripts three scoped workers and a director. L1's pool-wait
discovery is forwarded to L2 by trace ID. L2 preserves its note, source reference
and question in a checkpoint artifact before account A fails. The daemon
reconciles that worker, blocks B until the director grants access, then lets a
replacement on allowed account B read the checkpoint on attempt 2.
L1 and L3 remain active. Three accepted findings gate a final timeline that
labels causality as plausible rather than proven. A separate no-database test
blocks when only an unqualified cheap account and an unselected account remain,
survives daemon restart, and records zero unchanged-state wakes.

Routing snapshots and director decisions are scripted. A separate daemon
fixture tests checkpoint revocation during the replacement attempt.
The fixture does not prove live Auto Mode telemetry, live account handoff,
credential isolation or sandboxed read-only execution.
