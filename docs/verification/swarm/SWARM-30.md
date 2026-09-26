# SWARM-30 — director replacement

Status: partial. Revision: `0fc1446`.

Input: a fixture-admitted worker with a reserved quota window reports a discovery. The director claims it, and the daemon restarts before the batch is applied. The worker sends a late terminal result while director termination is uncertain. The fixture then reports confirmed director death.

Expected: uncertainty holds new coordination and keeps usage reserved; confirmation advances the director generation, requeues unapplied messages, keeps worker results, and rejects old director commands.

Actual: the run enters `stalled` on unknown termination. Claiming another director batch and sending an old directive fail; the worker result remains durable and its reservation stays active. Confirmed death increments generation from 1 to 2. The replacement claims both the original discovery and the late result, while the original completion token/directive is rejected. The focused test first failed because recovery did not exist, then passed after the transition was added. The workspace suite passed with 68 tests at revision `0fc1446`.

Evidence: `daemon/tests/swarm_director.rs`, `docs/verification/swarm/milestone-11.md`.

Remaining: the fixture supplies termination evidence; no live process identity/liveness proof, replacement target selection, model turn, or unavailable-replacement recovery is implemented. This is not proof that a real director can be safely killed or resumed.
