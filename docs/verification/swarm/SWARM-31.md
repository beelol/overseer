# SWARM-31 — asynchronous launch is not completion

Status: partial. Revision: `6152228`.

Input: the same local scripted worker launches and remains active; a caller attempts to mark its logical attempt finished before process exit. The daemon restarts and replays the launch request. A second fixture replays a pending launch after Stop, and a third tries to launch an already finished attempt.

Expected: a launch receipt cannot become a result or release the slot. Replay does not create a duplicate worker; Stop or a finished attempt cannot start a delayed worker. Only a confirmed terminal process state permits exit confirmation.

Actual: the tests first exposed an absent launch bridge, a pending launch that could continue after Stop, a linked worker counted twice against the global limit, and a finished attempt that could launch. After fixes, all three runtime tests pass. The normal worker stays `reserved` until an explicit result and review; launch alone cannot accept it. The workspace suite passed 81 tests.

Evidence: `daemon/tests/swarm_runtime.rs`, `daemon/src/swarm/runtime.rs`, `daemon/src/swarm/admission.rs`, `daemon/src/swarm/artifacts.rs`.

Remaining: no native asynchronous harness receipt, stale/silent-output sampler, late output append, parent-before-descendant ordering, or once-only native usage reconciliation is tested. This criterion remains unchecked.
