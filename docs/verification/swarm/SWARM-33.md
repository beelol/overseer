# SWARM-33 — bounded director batches and durable result review

Status: partial.

Input: a fixture director claims a mixed batch containing one routine progress message and one submitted result. It completes the turn without reviewing the result, then the daemon is killed and restarted. The plan gains an unrelated job; the director reclaims the result, accepts it against a stored artifact, and completes its turn. The first turn's completion receipt is replayed after this later review.

Expected: the routine message advances once, but delivery alone cannot erase the result's only review notification. A decision covering that result sequence permits application. Restart and repeated turn completion must not duplicate the decision.

Observed: the new test failed against the previous behavior because both messages were marked applied. The director now applies the progress message and requeues the undecided result in the same transaction that completes its turn. After restart and an unrelated plan revision, only the result is redelivered; an evidence-backed decision applies it, leaving one stored result and one decision. Replaying the first turn returns its original one-applied, one-deferred receipt. An upgrade test preserves duplicate receipt counts for older completed turns. The earlier 20-message and 32 KiB batch tests still pass. The full offline workspace suite passed 214 non-ignored tests, with 11 ignored; the final unchanged-job revision case passed separately after the full run.

Evidence: `daemon/tests/swarm_director.rs::unreviewed_result_returns_to_director_after_batch_completion_and_restart`, `daemon/src/swarm/schema.rs::old_completed_director_turns_keep_their_duplicate_receipt_count`, `daemon/src/swarm/director.rs`, and `daemon/src/swarm/completion.rs`.

Remaining: these are scripted daemon fixtures. There is no live director model turn or qualified provider reservation. An undecidable terminal report remains queued and can stall after two no-progress turns; the director needs an explicit durable unresolved disposition before live use. Stop remains immediate through the existing control path.
