# CONTRACT-05 — one launch through crash and reconnect

Status: in progress (Swarm booking recovery on `claude/auto-swarm`; see the last section)
Tested implementation commit: none; branches are separate.
Verification date and verifier: 2026-09-26, Swarm implementing agent (contract audit only).
OS / architecture / VS Code / harness versions: pin when integrated test runs.
Harness, provider and redacted account identities: local synthetic harness; no live account.
Prerequisites and fixture: shared durable launch phases, allowance commitment, workspace ownership and supervised child identity.
Steps or exact reproducible commands: kill the daemon before commitment, after commitment, after external worktree effect, after child-row commit and after process start. Reconnect two clients and replay the same work-unit ID; reconcile confirmed settlement versus uncertain effects.
Expected result: one intent/commitment and one child or a visible uncertain state; no second writer, process or model request; release only after confirmed settlement.
Actual result: not run across modes. Both branches have partial local crash fixtures but no shared transaction. Auto's launch-boundary design explicitly leaves full admission and reconciliation open.
Evidence paths: `daemon/tests/swarm_runtime.rs`, `docs/verification/swarm/SWARM-22.md`, `docs/rfcs/swarm-auto-contract.md` (partial precursors only).
Live vs fixture coverage: fixture precursor; no integrated or live coverage.
Known limitations and remaining platform/account combinations: owner proof for external Git effects, native process settlement and live adapter recovery unqualified.
Blocker, attempted alternatives and next action: keep unchecked; implement shared phases and run the full crash matrix with two clients.

## Swarm bookings across restart — 2026-09-27

Implementation commit: `dc8b37aa` on `claude/auto-swarm`. Fixtures only.

- A booked Swarm worker whose attempt is still registered keeps its booking across
  `kill -9` and restart (startup releases every other booked-but-unclaimed launch),
  so dispatch recovery can still launch it and the slot is held once throughout
  (`swarm_worker_admission_books_the_shared_account_and_binds_its_run`).
- A booking whose attempt is no longer registered (cancelled, failed or finished
  before its launch claimed effects) is released with its slot and account
  commitment, at startup and in every Swarm admission
  (`release_orphaned_swarm_bookings_in_tx`; unit test
  `one_count_takes_each_occupant_once_and_a_director_booking_takes_no_second_slot`).
- The worker launch claims the booking's effects before its worktree and binds its
  run in the transaction that inserts the run; a launch that finds the booking
  already claimed or released refuses to act (no second effect). The run's end
  settles the slot and retains the draw.
- In-flight ordinary starts' durable slot holds are cleared at startup: their
  requests died with the daemon.

Not yet shown: a crash between a Swarm worker's effects claim and its binding, two
reconnecting clients replaying a Swarm launch, Git-resource ownership, and live
adapter recovery. Keep unchecked.
