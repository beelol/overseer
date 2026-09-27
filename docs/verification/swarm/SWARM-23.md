# SWARM-23 — visible Swarm status

Status: partial. Initial revision: `0997e20`. Latest evidence revision: `6ba3d06`.

Input: a category with one admitted local worker, and a separate category with a planned job but no worker process. Both use the deterministic `OVERSEER_NOTIFY_COMMAND=/usr/bin/true` fixture. The tests call `daemon.background_notice`; the active-worker case is `daemon_stop_all_preserves_unconfirmed_swarm_worker_after_control_loss`, and the queued case is `background_notice_names_queued_swarm_without_a_worker_process` in `daemon/tests/swarm_control.rs`.

Expected: the background notice identifies the category as one Swarm, gives its active worker count, and does not present each worker as an unrelated agent. Queued category work remains visible even with zero worker processes. Ordinary-agent notifications retain their existing wording.

Actual: before the patch, the active worker was announced as one unrelated generic agent and the queued Swarm produced no notice. Both cases now announce one category-level Swarm, with respectively one and zero active workers; the notice payload retains the underlying run IDs and supplies category, status and worker count. The ordinary-agent regression `ac45_last_vscode_window_closing_with_active_runs_posts_a_notice_but_a_reload_does_not` passed. The full offline Rust suite passed 191 tests with 11 ignored using `cargo test --workspace --offline -q`; `git diff --check` passed.

Remaining at the notification revision: the VS Code extension lacked the required Swarm active/queued counts, targets/accounts, limiting constraint, usage state, finishing reserve and decision explanation. A background notification cannot verify those on-screen requirements. This criterion stayed unchecked.

At code revision `e773082`, `swarm.get` also gained per-status job totals and separate registered-attempt and running-supervisor counts; see [SWARM-37](SWARM-37.md). These fields are read APIs, not on-screen evidence, so the remaining UI requirements and this partial status are unchanged.

Sidebar follow-up (this revision): the extension now consumes bounded `swarm.list` summaries and renders active, ready and blocked counts in an expandable Swarms section of the Agents tree. Its unit fixture verifies those rows and lazy filtered job pages; the existing packaged sidebar scenario still passes. The fixture does not establish an on-screen account/usage/limiting-constraint/finishing-reserve readout, and no full Swarm UI scenario has run. SWARM-23 remains partial.

An isolated packaged-UI scenario subsequently displayed a 100-ready-job category in the real VS Code Agents sidebar, first compact and then expanded to its director and jobs. The screenshot and result are in `docs/verification/evidence/ui/swarm-status/`. It still has no account/usage/limiting-constraint/finishing-reserve readout or live worker process, so SWARM-23 remains partial.

At `0561034` (final evidence `a9232f2`), the packaged `scenario-swarm-scale.js` starts 32 supervised local fixture workers in a 100-job category. The Agents sidebar shows one category with 32 working and 68 ready, while hiding the 32 linked worker tasks from the ordinary agent list. Expanding one working job shows its linked worker and opens that worker's run view. This is visible state backed by the daemon's separate process count and 32 live supervisor child PIDs, not a claim about model-provider capacity. The screenshot/result set is `docs/verification/evidence/ui/swarm-scale/`. Account usage, the limiting constraint, finishing reserve and a decision explanation are still absent from this packaged UI. SWARM-23 stays partial.

Capacity readout (`9142d2d`): `swarm.get` and bounded `swarm.list` summaries now read the selected targets, frozen pool/window allocations, finishing reserves, and outstanding estimated commitments from durable admission tables. The readout explicitly says provider usage is unknown; it does not convert the synthetic fixture's `exact` window input into a measured live balance. The expanded packaged Agents sidebar names the selected fixture target, its 100-point allocation, 20-point finishing reserve, the recorded parallel-planning reason, and unknown current admission limit/usage while 32 supervised local workers run. The evidence is `docs/verification/evidence/ui/swarm-scale/03-capacity-expanded.png`, `result.json`, and `scenario.log`; the 100-ready-job control regression is in `docs/verification/evidence/ui/swarm-status/`. The focused Rust test was first red for the missing capacity field, then passed and survived a daemon restart. The sidebar unit test was red before rendering and green after. `cargo test --workspace --offline -q`, `node test/unit/run.js` (4/4), `npm run check --prefix extension`, `node extension/scripts/package.js`, `node test/ui/scenario-swarm-scale.js`, `node test/ui/scenario-swarm-status.js`, and the ordinary sidebar scenario all passed. The packaged scale run confirmed all 32 child PIDs exited after Stop.

Remaining: a live Auto producer and shared admission authority must supply fresh measured/estimated/stale account usage and persist the actual limiting admission reason. The current readout honestly labels those unknown. Logs and read APIs need a joined credential-redaction check; serial, scaled-down and blocked decision explanations still need packaged UI scenarios. SWARM-23 stays unchecked.

Admission-decision follow-up (`6ba3d06`): the first daemon test failed because
`swarm.get.capacity` had no record of a held admission. The daemon now stores
the latest blocked or admitted result for each run with its job, target, reason
and observation time, without storing the worker token. The fixture holds a
job on an unknown target, restarts the daemon, and verifies that the hold
survives; a successful admission replaces it, while an idempotent replay does
not. This optional readout is written after the admission transaction, so a
readout-write failure cannot turn a committed reservation into a false launch
failure. The sidebar labels the result **Last admission**, with a tooltip
stating that current eligibility may have changed.

The packaged 32-worker scenario deliberately tries one extra ready job after
the director and workers occupy their slots. It receives `worker_limit`
without a new attempt, and the expanded Capacity row shows “Last admission
held · worker limit” with that job and target. The screenshot is
`docs/verification/evidence/ui/swarm-scale/04-capacity-expanded.png`; the same
run's ordered checks and process cleanup are in `result.json` and
`scenario.log`. `cargo test -p overseerd --bin overseerd --test
swarm_admission --test swarm_scheduler --test swarm_dispatch --offline --quiet`
passed 82+35+6+4 tests, `node test/unit/run.js` passed 5/5,
`npm run --silent check --prefix extension` passed, the VSIX packaged, and
`node test/ui/scenario-swarm-scale.js` passed. The first packaged attempt was
invalid because the probe omitted the fixture director's owner token; it was
corrected and rerun, with no worker process left behind.

The last hold is a past daemon decision, not a continuously recomputed current
limit. A live Auto producer, fresh account usage, selected account labels,
serial/scaled-down/blocked explanations across the full UI matrix and joined
credential-redaction evidence remain open. SWARM-23 stays partial.

Blocked and scaled-down UI follow-up (`f4c6f4f`): the Capacity section now shows
the last durable eligibility observation and its reason, marking an expired
observation as expired rather than current. The planning row shows the recorded
parallel-worker cap against the run's effective worker ceiling. The unit test
first failed because the blocked eligibility row was absent, then passed after
the renderer change; it also checks an expired observation and a three-of-eight
parallel decision. A packaged VS Code fixture with 100 ready jobs, a four-slot
app limit, and no approved target recorded a beneficial batch capped at three
workers plus a `no_allowed_target` observation. The expanded Capacity row showed
both facts. Screenshot, ordered checks, and log are under
`docs/verification/evidence/ui/swarm-status/` (`03-swarm-blocked-capacity.png`,
`result.json`, `scenario.log`). The screenshot was visually inspected. The
packaged scenario still passed its existing deadline, Pause, Resume, Off, and
Stop checks. `node test/unit/run.js` passed 5/5, the extension source check and
VSIX build passed, and `git diff --check` passed. This is fixture evidence of
past decisions, not proof of a continuously current account constraint.

The same packaged fixture then increased the estimated coordination cost for
the same four jobs. The daemon recorded `serial` with `no_time_benefit` in a
second wave, and a refreshed Capacity section showed that reason and one of
eight workers. Its visually inspected screenshot is
`docs/verification/evidence/ui/swarm-status/04-swarm-serial-capacity.png`;
`result.json` records both planning inputs and observed UI rows. No model or
provider account was used.

Remaining: Auto integration must provide selected live account labels and
measured/estimated/stale usage with a fresh limiting constraint. The combined
status/log surface needs credential-redaction evidence. SWARM-23 remains
partial and its RFC box stays unchecked.
