# CONTRACT-01 — shared atomic admission

Status: in progress (step 3 of the handover on `claude/auto-swarm`; see the last section)
Tested implementation commit: none; the Auto and Swarm branches are separate.
Verification date and verifier: 2026-09-26, Swarm implementing agent (contract audit only).
OS / architecture / VS Code / harness versions: not applicable to the proposed contract; pin versions when the integrated test runs.
Harness, provider and redacted account identities: synthetic shared pool and two synthetic clients; live identities not yet exercised.
Prerequisites and fixture: one daemon-owned admission service used by ordinary, Auto and Swarm launch paths, with a known binding short/long account window and one workspace writer.
Steps or exact reproducible commands: race two different callers for the last allowance and writer; change account generation after route selection; replay the winning work-unit ID from two clients. Run the integrated daemon test and `cargo test --workspace --offline -q -- --test-threads=1` at one pinned revision.
Expected result: one commitment, slot, writer and durable intent; the loser waits or pauses with a specific reason; changed identity requires revalidation; replay cannot reserve twice.
Actual result: not run. Swarm fixture admission revalidates injected snapshots and shares an app agent-slot lock with ordinary starts, but account allowance is not shared with Auto or ordinary starts.
Evidence paths: `daemon/tests/swarm_admission.rs`, `docs/rfcs/swarm-auto-contract.md` (partial precursor only).
Live vs fixture coverage: fixture precursor; no integrated or live coverage.
Known limitations and remaining platform/account combinations: Auto producer and atomic cross-mode allowance commitment are absent on this branch.
Blocker, attempted alternatives and next action: keep unchecked; integrate one reservation authority with Auto and run the concurrent multi-window test.

## Read-only integration check — 2026-09-27

Inputs: Swarm `2927b431` and the locally active Auto branch `68dea8f4`;
published `origin/main` remains `b84f9984` and is already in Swarm. Commands:
`git merge-base HEAD codex/automode-rfc`, `git merge-tree <base> HEAD codex/automode-rfc`,
and a count of conflict markers by merged file. The dry merge has 25 conflict
hunks in seven files: `adapters.rs` (2), `daemon.rs` (9), `git.rs` (3),
`server.rs` (3), `store.rs` (6), the daemon test helper (1), and the
extension entry point (1). The branches also overlap without a textual
conflict in six more files. This is a compatibility forecast, not an
integrated build or a merge into either active branch.

The normal Swarm path still cannot satisfy this contract: `swarm.create` is
public, but native director launch, planning, admission and dispatch remain
fixture-gated, and the scripted director launch does not make a shared account
allowance commitment. Auto's in-flight collector and route code must be
consumed at the shared transaction; copying its collector or simply removing
the fixture gate would give a false S0 launch. Resolve the overlapping daemon
entry points when Auto's integration surface is stable, then make the first
joined test race an ordinary start, Auto child and Swarm worker against one
binding short and long window and one writer. No CONTRACT-01 or SWARM-01 box
is checked by this audit.

## Disposable integration attempt — 2026-09-27

Inputs: Swarm `8b66612b` and the current local Auto `dcf8da7b`. A separate
`codex/swarm-auto-integration` worktree was created from the Swarm head and
`git merge --no-commit --no-ff dcf8da7b` was run there. Neither active branch
was changed. The merge stops at 25 conflict hunks in seven files: `adapters.rs`
(2), `daemon.rs` (9), `git.rs` (3), `server.rs` (3), `store.rs` (6), the daemon
test helper (1), and the extension entry point (1). Auto's local branch now
includes main `7692f4fe`; its published branch is still older at `c7693836`.

The conflicts are behavioral, not just formatting. Auto's `Store` now has a
separate learning connection and schema version 18; Swarm has its own main-DB
schema migration, write-capacity probe, run-slot and supervisor links. The
combined store must retain Auto's learning isolation while migrating Swarm's
durable tables in the main connection. Auto's `auto_pool_claims` serializes its
own root/child unknown-draw launches; Swarm's `swarm_reservations` uses injected
allowance windows. Those are still two authorities and cannot satisfy the
ordinary/Auto/Swarm race by merely resolving source conflicts. The shared
transaction must bind account generation, every applicable window, app slot,
workspace writer and launch intent before a process or Git effect. A changed
generation and an uncertain prior effect must remain blocking in that test.

The first joined test should race one ordinary start, one Auto child, and one
Swarm worker for the last capacity in a shared short and long window and one
writer, then replay the winner after restart. Until that test passes at one
integrated revision, keep CONTRACT-01 and SWARM-08/24 partial. Do not copy
Auto's collector or lift Swarm's fixture gate to make the merge appear usable.

## Step 3 on `claude/auto-swarm`: one app-slot authority, Swarm on the booking — 2026-09-27

Implementation commit: `dc8b37aa` (after merge `a87da0fa`). Fixtures only: every
quota observation is a fixture row or the Codex app-server fixture, and no
subscription allowance was spent or measured.

What now holds, and the test that shows it:

- **One app-slot count.** `account_booking::app_slots_in_use` is the only count of
  `agents.max_active` occupants, read inside the admission transaction of every
  path: ordinary starts and follow-ups (a durable `app_slot_holds` row replaces the
  former in-memory pending counter and is deleted in the commit that inserts the
  run), Auto roots and children (`insert_auto_root_selected`,
  `insert_auto_selected_decision`, now IMMEDIATE), booked starts
  (`book_shared_launch_in_tx`) and Swarm admission (now IMMEDIATE). Each occupant
  is counted once: a hold, a slot-holding booking, a pending Auto child, a
  registered Swarm worker attempt without a slot-holding booking, an active
  category's director slot, or any other run that may still own a process.
  Overseer's own coordinating run holds no slot, as this contract says.
  Unit test `one_count_takes_each_occupant_once_and_a_director_booking_takes_no_second_slot`.
- **Four paths race for the last slot; exactly one wins.** Daemon test
  `ordinary_auto_swarm_and_booked_starts_race_for_the_last_slot_and_one_wins`
  (`daemon/tests/shared_launch.rs`): a running category holds two of three slots;
  an ordinary `task.create`, an `auto.start` root, a Swarm `swarm.admit` and a
  fixture-booked `task.create` are sent at once on four connections. Exactly one
  wins and each other is refused for the agent limit. After `kill -9` and a
  restart the winner still holds its slot while its run or attempt lives (a late
  start is refused), and a run winner's slot is released exactly once when it
  ends. Unit test `four_admission_paths_race_for_the_last_slot_and_exactly_one_wins`
  races an ordinary hold, a booked launch, an Auto child and an Auto root on four
  SQLite connections.
- **Swarm books through the shared booking.** Daemon test
  `swarm_worker_admission_books_the_shared_account_and_binds_its_run`: admitting an
  account (Claude) target books `swarm/<attempt>` with `book_shared_launch_in_tx`
  inside Swarm's own admission transaction, holding the worker's slot (the attempt
  is not counted again) and drawing each cited window in thousandths of a reported
  percentage point. `allocation_remaining_milli` is the category's remaining
  allocation in those same windows, so a draw above it is refused
  `allocation_exhausted`; with no upper draw the admission is refused
  `upper_draw_unknown`. An ordinary booked start on the same account sees the
  Swarm draws and is refused `shared_pool_headroom`. `swarm_reservations` gets no
  row for a booked worker.
- **A director's linked run takes no second slot.** In the unit test, a director
  run bound to a booking made with `consume_agent_slot: false` is counted once
  while its category plans and once (as the category's director slot) after it
  runs.

Still open, so this stays unchecked:

- The qualified per-window upper draw (`b08646f8`, `daemon/src/upper_draw.rs`)
  lets a Swarm account worker and a `draw: "qualified"` booked start book without
  fixture inputs once five isolated runs of the same harness, model and effort
  exist on the account (`qualified_draw_admits_a_swarm_worker_after_five_isolated_runs`,
  `qualified_draw_prices_a_booked_start_after_five_isolated_runs`); before that
  they are refused `upper_draw_unknown`. Its readings in those tests are fixture
  meters, and no live window delta has been attributed.
- Auto roots and children still hold unknown-draw account claims rather than
  per-window bookings, so the account-window race is shown for booked ordinary and
  Swarm callers, not for Auto (Auto competes for the app slot only).
- The writer race: a Swarm worker gets a new worktree, so it books no writer, and
  no Swarm caller competes for an existing checkout yet.
- A changed account generation between route and launch is refused by the booking
  (the booking's own unit tests) but has not been replayed with a Swarm caller.
- Scripted directors run the generic harness and book nothing; the
  `consume_agent_slot: false` director path is shown in the count, not through a
  director launch. Gate S's Overseer-started agents and watchers use `task.create`
  and so the same count, but no Gate S-specific race was run.
- Short and long live windows, and live providers.
