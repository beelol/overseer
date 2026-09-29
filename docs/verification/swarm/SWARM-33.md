# SWARM-33 — bounded director batches and durable result review

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section.

Input: a fixture director claims a mixed batch containing one routine progress message and one submitted result. It completes the turn without reviewing the result, then the daemon is killed and restarted. The plan gains an unrelated job; the director reclaims the result, accepts it against a stored artifact, and completes its turn. The first turn's completion receipt is replayed after this later review.

Expected: the routine message advances once, but delivery alone cannot erase the result's only review notification. A decision covering that result sequence permits application. Restart and repeated turn completion must not duplicate the decision.

Observed: the new test failed against the previous behavior because both messages were marked applied. The director now applies the progress message and requeues the undecided result in the same transaction that completes its turn. After restart and an unrelated plan revision, only the result is redelivered; an evidence-backed decision applies it, leaving one stored result and one decision. Replaying the first turn returns its original one-applied, one-deferred receipt. An upgrade test preserves duplicate receipt counts for older completed turns. The earlier 20-message and 32 KiB batch tests still pass. The full offline workspace suite passed 214 non-ignored tests, with 11 ignored; the final unchanged-job revision case passed separately after the full run.

Evidence: `daemon/tests/swarm_director.rs::unreviewed_result_returns_to_director_after_batch_completion_and_restart`, `daemon/src/swarm/schema.rs::old_completed_director_turns_keep_their_duplicate_receipt_count`, `daemon/src/swarm/director.rs`, and `daemon/src/swarm/completion.rs`.

Remaining: these are scripted daemon fixtures. There is no live director model turn or qualified provider reservation. An undecidable terminal report remains queued and can stall after two no-progress turns; the director needs an explicit durable unresolved disposition before live use. Stop remains immediate through the existing control path.

At `52eea97`, a versioned Atlas J2 probe precedes 2,000 duplicate progress messages. A concurrent Stop is acknowledged in 7 ms on the recorded machine, leaving one progress envelope and no acceptance decision; see [S5](S5.md). This strengthens local Stop priority evidence but does not qualify a live director turn.

## Verified at fixture scope (2026-09-28)

The earlier batch test used twenty *progress* events and Stop only after the turn had completed. `twenty_results_are_one_review_turn_and_stop_does_not_wait_for_it` (`daemon/tests/swarm_director.rs`, new; passed on first run) joins the clause in one fixture:

| Clause | What the test shows |
| --- | --- |
| Batch 20 routine completion events into one director review turn | twenty workers' `result` envelopes are one claimed batch; the director accepts all twenty inside that turn |
| Stop prevents new dispatch immediately, without waiting for the model turn | Stop, sent while the turn is still open, returns in under 2 s; a new attempt and an admission for the queued job are refused; later claims are `halted` |
| Events persist through restart | the daemon is killed with the turn open; afterwards the turn is still active (`busy`) and its twenty results are reviewed |
| Stale plan revisions are rejected | a claim at revision 0 fails `stale plan revision` |
| No result dropped or processed twice | completing the turn applies twenty notifications; a repeated completion is a duplicate; 20 decisions, 20 applied results, no attempt for the queued job |

Related tests kept: `director_batches_twenty_events_and_never_claims_two_active_turns` (21 events → 20 + 1, one active turn across restart), `inline_batch_limit_defers_excess_and_stop_halts_future_turns` (32 KiB), `unreviewed_result_returns_to_director_after_batch_completion_and_restart`, `stop_wins_over_a_late_no_progress_turn_completion`, and the Atlas flood `atlas_s5_two_thousand_duplicate_progress_messages_do_not_starve_stop`. `swarm_director` passed 12 tests serially.

Boundary: the "model turn" is a scripted claim and completion; a live director's turn length and reasoning are not measured here.
