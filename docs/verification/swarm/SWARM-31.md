# SWARM-31 — asynchronous launch is not completion

Status: partial. Revision: `c37ca47`.

Input: the same local scripted worker launches and remains active; a caller attempts to mark its logical attempt finished before process exit. The daemon restarts and replays the launch request. A second fixture replays a pending launch after Stop, and a third tries to launch an already finished attempt.

Expected: a launch receipt cannot become a result or release the slot. Replay does not create a duplicate worker; Stop or a finished attempt cannot start a delayed worker. Only a confirmed terminal process state permits exit confirmation.

Actual: the tests first exposed an absent launch bridge, a pending launch that could continue after Stop, a linked worker counted twice against the global limit, and a finished attempt that could launch. After fixes, four runtime tests pass. The normal worker stays `reserved` until an explicit result and review; launch and terminal process status alone cannot accept it. The daemon's bounded background scan now finds a terminal linked process, writes one neutral event to the director inbox with the attempt's original revision, and only then confirms exit. The deadline test proved this happens without a manual reconciliation call. The combined-main workspace suite passed 102 tests.

Evidence: `daemon/tests/swarm_runtime.rs`, `daemon/src/swarm/runtime.rs`, `daemon/src/swarm/admission.rs`, `daemon/src/swarm/artifacts.rs`.

Remaining: no native asynchronous harness receipt, stale/silent-output sampler, late output append, parent-before-descendant ordering, or once-only native usage reconciliation is tested. This criterion remains unchecked.

Control receipt collision follow-up: a registered worker could previously submit a progress message using the daemon's future `terminal-att-...` ID. Terminal reconciliation then found the ID with a different payload and could not confirm exit; the same technique could preempt the `stop-att-...` control message inserted with `INSERT OR IGNORE`. The focused `admitted_worker_launch_replays_to_one_supervised_run_after_daemon_restart` test reproduced the accepted spoof before the fix, then passed after broker validation reserved the daemon-generated terminal, Stop, deadline-checkpoint, and contamination ID shapes. The full `swarm_broker` suite passed (18 tests), including ordinary `terminal-0` report IDs. These are fixture-only control guarantees; the native receipt and descendant cases above remain open.
