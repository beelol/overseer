# Swarm implementation milestone 11 — director recovery state

A fixture-only recovery transition distinguishes an unreachable director from confirmed termination. Unknown termination moves the run to `stalled`, where coordinator mutations and new admissions are held while worker reports still persist. Confirmed death closes the old active turn, requeues its delivered-but-unapplied messages, advances the run generation and permits a replacement to claim the backlog. Existing worker reservations are not released by director replacement.

The test restarts the daemon between batch delivery and acknowledgement, reports a worker result during the stall, then confirms that the next director sees both messages. Stale generation commands fail. The recovery RPC remains disabled in the default daemon because it currently accepts fixture-supplied termination evidence; a live implementation must verify the process before using it.

Evidence: `daemon/tests/swarm_director.rs`. SWARM-30 is partial. Actual director model turns, replacement eligibility, process liveness and unavailable replacement behavior remain to be implemented.
