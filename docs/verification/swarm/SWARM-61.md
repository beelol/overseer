# SWARM-61 — Stop ordering around local launch

Status: partial. Revision: `6152228`.

Input: a fixture-admitted attempt has a durable but unlinked launch intent. Stop commits before the request is replayed. Separately, Stop reaches a linked `/bin/sleep` worker after a daemon restart.

Expected: an unlinked request cannot spawn after Stop; a linked worker is interrupted without treating the interrupt request as confirmed exit.

Actual: the red pending-intent fixture launched after Stop. The fixed path rechecks run and job state before continuing and starts no process. The linked worker receives an interrupt through Overseer's existing supervisor and reaches a terminal run state; the attempt is finished only after that state is observed. The workspace suite passed 81 tests.

Evidence: `daemon/tests/swarm_runtime.rs`, `daemon/src/swarm/runtime.rs`, `daemon/src/server.rs`.

Remaining: permission revocation, result-vs-Stop acceptance races, queued retry of initially unreachable processes, native descendants, and explicit user resume/extension are not covered. This criterion remains unchecked.
