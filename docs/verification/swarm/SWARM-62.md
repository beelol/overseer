# SWARM-62 — run deadline

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. First revision: `c37ca47`.

Input: two runs with a 1.5-second fixture deadline. One has a registered attempt and a queued job; the other has no approved target. Neither receives another admission request. A separate case presents an expired admission timestamp.

Expected: the daemon expires both runs without a model wakeup, identifies deadline as the reason, cancels queued work and requests a stop for active work.

Actual: both runs enter `stopping` within the bounded test wait, expose `stop_reason: deadline`, and distinguish `cancel_requested` active work from `cancelled` queued work. The active attempt receives a queued Stop directive. A new fixture launches a supervised local process and then sends no more admissions; the daemon deadline timer interrupts the process, observes its terminal state, delivers a neutral director event, and marks the job cancelled. The admission-triggered path records the same reason. The combined-main workspace suite passed 102 tests.

Follow-up revision `f91fa58`: the supervised-worker fixture sends ten progress messages during a 2.5-second run. The persisted creation time and effective deadline do not move, the daemon still interrupts the worker for `deadline`, and the job is cancelled with one attempt. The full Rust suite passed 128 tests.

At `f6886d0`, `swarm.get` now exposes linked workers whose exits remain unconfirmed while a run is stopping, including the last Stop-signal outcome and bounded detail count. A Stop fixture proves the readout across a simulated failed signal, restart, and confirmed exit. The deadline-specific fixture was not extended to observe this readout; that path remains partial evidence.

Evidence: `daemon/tests/swarm_control.rs`, `daemon/tests/swarm_state.rs`, `daemon/tests/swarm_runtime.rs`, `docs/verification/swarm/milestone-10.md`.

Remaining: the generic local process is not a qualified live harness, and native descendants are not covered. Deadline-specific unconfirmed-exit readout, UI display, and an explicit extension that preserves the original account allocation remain unverified.

Blocked-deadline follow-up at `4bb688b`: a new Atlas S5 replay starts a supervised J4 worker whose actual PostgreSQL-backed attachment probe returns 200 before holding the command. A newer fixture observation removes all allowed targets and holds queued J2. The original eight-second run deadline then queues a durable checkpoint request and Stop, interrupts J4, cancels J2 and refuses completion. The checkpoint request is not a confirmed artifact, and live harness/descendant control, deadline-specific UI, and explicit extension remain unverified. The focused deadline regression failed before the checkpoint change and passed after it; all 15 disposable Atlas tests plus six control, 18 broker and one non-ignored scenario test passed.

Related job-deadline regression at `f831eac`: a local director's admitted job expires
at a daemon-restart boundary. The deadline timer retries interruption of the linked
director process after restart, and confirmed-dead recovery fails the job without a
second attempt. This is a **job** deadline, not SWARM-62's whole-run deadline; it does
not verify the run-deadline UI, live harness, native descendants or extension path.
SWARM-62 remains partial.
Replay: `cargo test --offline -q -p overseerd --test swarm_director_loop -- --test-threads=1`.

Explicit extension follow-up at `ad74547` (fixture and packaged UI): a run starts
with a 60-second deadline, three queued jobs, and one admitted job that freezes a
synthetic `points` allocation and finishing reserve. An owner request adds 60 seconds
using the observed deadline as a compare-and-set value and a stable request ID.
Expected: one durable extension, no extra account allocation, no deadline Stop at
the original boundary, and a deadline Stop at the new boundary. Actual: the
extension is recorded in `swarm_deadline_extensions`, replay returns `duplicate`,
changed-input replay and stale-deadline requests are rejected, and the updated
policy survives daemon restart. An admission probe beyond the original deadline
is blocked by the separate benefit gate, **not** `run_deadline`; a probe at the
new deadline returns `run_deadline` and records Stop. The frozen allocation and
reserve are byte-for-byte unchanged. A new extension after Stop is rejected.

In isolated packaged VS Code, the category row's **Extend Swarm Deadline…**
action adds 30 minutes (effective deadline 3,600,000 → 5,400,000 ms) and records
`run_extension` as its source. The same scenario still passes Pause, Resume, Off
and confirmed Stop. Evidence: `daemon/tests/swarm_admission.rs`,
`test/unit/swarm-controls.js`, `test/ui/scenario-swarm-status.js`, and
`docs/verification/evidence/ui/swarm-status/result.json` plus `scenario.log`.
Replay: `cargo test --offline -q -p overseerd --test swarm_admission --test swarm_control --test swarm_settings --bin overseerd -- --test-threads=1`;
`cargo test --workspace --offline -q -- --test-threads=1`;
`node test/unit/swarm-controls.js`; `npm run check` in `extension`;
`node extension/scripts/package.js`; `node test/ui/scenario-swarm-status.js`.
The full offline Rust suite, focused fixture, extension check, package and
packaged UI scenario passed. Unrelated regenerated TUI snapshots were restored.
SWARM-62 stays partial: deadline-specific unconfirmed-exit UI, qualified live
harness and native-descendant control, and a live blocked/active extension path
are still unverified.

## Verified at fixture scope (2026-09-28)

Earlier fixtures set short deadlines. `default_sixty_minute_deadline_stops_active_and_blocked_runs_but_nothing_else` (`daemon/tests/swarm_runtime.rs`, new; passed on first run) uses the built-in 60-minute deadline and moves only the runs' start times back by 60 minutes and 1 ms:

| Clause | What the tests show |
| --- | --- |
| The default 60-minute deadline expires while active and while blocked | both runs report `deadline_ms: 3600000` by default; the daemon's own timer stops the active run (`stopping`) and the blocked one (`stopped`), each with `stop_reason: deadline`; `deadline_expires_without_another_admission_while_active_or_blocked` and the joined `atlas_s5_run_deadline_expires_while_all_targets_are_blocked` (passed again) cover short configured deadlines |
| Checkpoint, cancel queued work, interrupt active work as supported | the queued job is `cancelled`; the active worker receives a `checkpoint` and a `stop` envelope and an interrupt request |
| Show unconfirmed exits | while the worker ignores SIGINT, `swarm.get` shows `unconfirmed_exit_count: 1` naming its run; after the worker is killed its exit is confirmed, the count is 0 and the run is `stopped` |
| Do not kill unrelated sessions or close VS Code | an ordinary agent on the same daemon and a third Swarm category keep running; the deadline acts only on the expired runs' linked workers (the daemon has no path that closes the editor) |
| An explicit extension is recorded and does not enlarge the account allocation | `explicit_run_deadline_extension_survives_restart_without_new_account_allocation` (passed) |

Rerun serially on 2026-09-28: `swarm_control` 6 passed, `swarm_admission explicit_run_deadline` 1, `swarm_runtime default_sixty` 1, the Atlas blocked-deadline replay against PostgreSQL 16.

Boundary: native descendants of a live harness and the editor's own display of the deadline are not part of this fixture proof (SWARM-25, SWARM-23).
