# Shared account booking: daemon integration seam

Status: partial implementation on `claude/auto-swarm`. The account booking landed at `17666b80`, the launch booking (slot, writer, effects claimed once) at `6abf8d71`, and run binding, settlement and startup recovery at `8d67c07a`. AUTO-AC-17, AUTO-AC-24 and Swarm CONTRACT-01/05 remain open.

The account authority is the daemon's existing `auto_pool_claims` table. The name is historical: a new `shared_booking_intents` row and its `shared_booking_windows` commitments attach to the same claim, rather than creating a second account balance. Existing Auto root/child claims continue to hold an entire account pool while their draw is unknown. A known-window booking checks those claims, active runs and open follow-up turns before it commits. Conversely, existing Auto/manual checks see a shared booking as an occupied account pool. No provider allowance is inferred from token counts or estimated credits.

`daemon/src/account_booking.rs` exposes `book_shared_account_in_tx(conn, request)` for callers that already hold an **IMMEDIATE** SQLite transaction, and `Store::book_shared_account(request)` when the booking can own that transaction. A request has a globally stable work-unit ID, caller and route, selected profile, quota-source profile, expected account generation, exact structured quota event, caller work hash, one qualified upper draw for **every** binding window, and optional remaining category allocation per window. The service re-reads the recorded account identities and latest structured quota event, checks freshness and every comparable window, subtracts active or uncertain commitments, then inserts the durable intent, pool claim and window commitments in one transaction. A repeat with the same full request returns `Replayed`; changed fields under one ID are rejected. Unknown, stale or unqualified draw blocks booking. A confirmed pre-effect failure may release a `booked` claim; marking effects uncertain makes it non-releasable by that path.

## Launch lifecycle

`book_shared_launch_in_tx` adds the app slot (`agents.max_active`, default 9) and the planned workspace writer to the same transaction and returns a durable intent. `claim_shared_launch_effects` lets exactly one worker act on it: the phase moves from `booked` to `uncertain` before any Git or process effect, so a reconnecting caller replays the intent instead of attempting the launch again.

| Step | Durable state | Slot | Writer | Account commitment |
| --- | --- | --- | --- | --- |
| Booked | intent `booked`, claim `active` | held | held (if a path was booked) | committed per window |
| Effects claimed | intent `uncertain`, claim `uncertain` | held | held | committed |
| Bound (`bind_shared_launch_run_in_tx`, in the transaction that commits the queued run row) | `run_id` set | held **by the intent only**: the run is excluded from the unbound-run count | held by the intent; the run is excluded from the unbound writer check | committed; the run is excluded from the whole-account "manual run" check because its draw is already in its windows |
| Run ends with its processes confirmed gone (`completed`, `failed`, `interrupted`, `unknown` after an exit record, or `disconnected` once supervisor and harness pids are both dead) | `settled_ms`, outcome `settled` | released | released | **retained** |
| A structured observation of the same account taken, in every window, after settlement | outcome `observed_after_settlement`, claim `released` | — | — | released |
| Run ended without any supervisor (first generation, `run_dir` never kept) | outcome `not_started`, phase and claim `released` | released | released | released (no model process ran) |

A bound run must still be queued with no supervisor identity, on the booked profile and, if a path was booked, on that workspace. Its supervisor identity is recorded before spawn (as for Auto roots and children), so an unbound claimed intent cannot have a model process and a bound run with no `run_dir` in its first generation never started one. A lost supervisor whose harness may still run keeps every hold. After settlement, a follow-up turn that reactivates the run is ordinary work again and counts as such. Occupancy is computed from the intent's flags **and** the bound run's status, so a run ended by a path that bypasses the settlement hook (a daemon stop marks runs interrupted directly) holds nothing and is settled at the next booking or restart.

Releasing a retained commitment on a later observation means the draw is now counted in that observation's reported usage, which every later booking must cite. It is not a qualified attribution of the draw to this work, and a provider's reporting delay could under-count it for a short time.

## Startup recovery

`Store::reconcile_shared_launches_on_start` runs before supervisors are reattached:

- **Booked, never claimed:** no effect was requested. Released (`released_unclaimed`); a replay is refused and must book a new attempt.
- **Claimed, no bound run:** the Git effect is unknown, and no model process can exist. The slot and the account commitment are released; the writer stays held and the intent is kept for reconciliation (`effects_uncertain`). The effects claim can never be won again, so a replay does not retry it. No automatic path yet releases that writer.
- **Bound run already ended** by any path: settled as above.
- **Bound run that may still own a process:** left to supervisor reconciliation. A live supervisor is reattached with its binding unchanged (still one slot); a run whose supervisor and harness are gone becomes `disconnected` and settles; a queued bound run that never got a supervisor is failed and releases everything.

The same release happens in-process: a booked ordinary start that stops before its effects claim releases everything, one that stops after the claim but before binding releases the slot and account commitment and keeps the writer.

## Callers

The ordinary manual start (`task.create`) books a shared launch only when the caller supplies a qualified booking, and only behind the fixture gate `OVERSEER_SHARED_BOOKING_FIXTURE_API=1`, because no product path can yet qualify an upper draw. It then claims immediately before its first Git effect, binds the run in the run's own commit and returns the bound run to a repeated or concurrent request. Without a booking, manual and Auto launches keep their existing unknown-draw guards; they now also refuse a checkout or root workspace whose writer a shared launch holds. Auto roots and children are not converted to known-window commitments: Auto has no qualified per-window upper draw yet, and its own unknown-draw claims already live in the same claim table. Swarm's fixture reservations are on its branch and have not been moved onto this booking.

The native unit is one thousandth of a reported percentage point, matching the structured `QuotaWindow.used_percent` source. It does not convert tokens, currency or estimated credits into that unit. Other native-unit meters need their own validated source projection. Cross-profile conflicting observations and proof that a learned upper draw applies to the selected account/model/window remain qualification work.

## Evidence

Unit tests (`daemon/src/account_booking.rs`): the original four (two-connection window race, linked-profile replay, stale identity/snapshot and pre-effect release, slot/writer/claim-once) plus `bound_run_is_counted_once_and_settlement_releases_its_slot_and_writer`, `binding_needs_the_effects_claim_and_the_booked_profile_and_workspace`, `a_bound_run_that_never_got_a_supervisor_releases_its_account_commitment`, `startup_recovery_releases_unclaimed_keeps_uncertain_writers_and_settles_finished_runs` and `two_connections_claim_one_booked_launch_exactly_once`.

Protocol tests against the real daemon, Git and the Codex app-server fixture (`daemon/tests/shared_launch.rs`): two concurrent identical booked starts produce one run and one model turn; with `agents.max_active=2` a running bound run and a second booked start both fit and a third is refused with `global_agent_limit`; the ended run releases its slot and writer but keeps its 30-point draw until a later observation; after `kill -9` of the daemon, a running bound run is reattached (one process generation, no second model turn, three reconnecting clients get the existing runs) and a run whose processes were killed settles; crash points after booking and after the effects claim recover as above and an ordinary writer on the held checkout is refused. Replacing the unbound-run exclusion with a no-op fails both count-once tests. All observations are fixtures; no subscription allowance was spent or measured.
