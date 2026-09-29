# SWARM-49 — bounded inbox and admission backpressure

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`, after `03235d4d`); see the last section. Earlier evidence revision: `ab29ff3`.

Input: one planned run with two ready jobs, a registered first attempt and 1,000 distinct progress reports. A second admission uses a fresh, compatible fixture quota snapshot. The active attempt then submits a terminal result.

Expected: routine progress is capped and deduplicated before director inference; new workers wait at the inbox threshold, but terminal events from in-flight workers remain durable.

Actual: `swarm.admit` returns `director_inbox_full` for the second job, and the first job's terminal result changes it to `submitted`. The existing broker test also rejects a 1,001st routine message and accepts a terminal result at capacity. The focused test initially failed because the second admission succeeded; it passed after the admission gate was added. The workspace suite passed with 67 tests at revision `042b3e5`.

Follow-up revision `a0ef330`: `stop_is_not_starved_by_two_thousand_duplicate_progress_replays` sends 2,000 duplicate progress envelopes over real local daemon socket connections while issuing Stop after the flood has begun. Every replay receives a duplicate receipt, the director inbox retains one event, and Stop returns `stopping` within the RFC's two-second bound. The full offline Rust suite passed 178 non-ignored tests. This measures the local fixture machine and does not qualify a live director model's context behavior.

Follow-up revision `66238db`: `malformed_and_unauthorized_reports_return_bounded_errors_without_inbox_effects` submits seven invalid envelopes, including wrong worker identity, missing/oversized message ID, a worker-spoofed director command, wrong revision, non-object payload and oversized payload. Every rejection is at most 160 characters, echoes neither the supplied token nor payload sentinel, leaves the director inbox empty, and does not prevent a later valid report. The focused test and full offline Rust suite pass (179 non-ignored tests). This exercises local broker authorization and diagnostics, not cross-harness delivery.

Evidence: `daemon/tests/swarm_admission.rs`, `daemon/tests/swarm_broker.rs`, `docs/verification/swarm/milestone-9.md`.

Remaining: live director inference/context limits and broader permission paths remain unverified. Stop responsiveness under this local duplicate flood is covered; broader load behavior is not.

At `ab29ff3`, `terminal_inbox_bypass_is_bounded_per_attempt_and_replays_stay_idempotent` first failed because a 17th result/submit/blocker report from one attempt was accepted with a fresh ID. The broker now permits at most 16 outstanding terminal reports per attempt. It withholds acknowledgement for overflow, preserves exact duplicate receipts, and admits a later correction after the director applies one report. The focused broker suite passed 17 tests; the full serialized offline workspace suite passed 205 tests with 11 ignored; `git diff --check` passed. This proves the local persistence and backpressure rule. Retry delivery from a real harness after an unacknowledged overflow and live director drain remain unqualified, so SWARM-49 is still partial.

## Verified at fixture scope (2026-09-28)

Each part of the Verify clause maps to a non-ignored test, rerun serially after `03235d4d` (`swarm_broker` 21 passed, `swarm_director` 11 passed, `swarm_admission full_director_inbox` 1 passed):

| Clause | Test |
| --- | --- |
| Malformed, oversized and unauthorized envelopes get a bounded diagnostic | `malformed_and_unauthorized_reports_return_bounded_errors_without_inbox_effects` (seven invalid envelopes, each rejection at most 160 characters, no echo of token or payload, no inbox effect) |
| 2,000 duplicates are deduped before inference, and Stop stays responsive | `stop_is_not_starved_by_two_thousand_duplicate_progress_replays` (one director envelope, Stop under 2 s over the local socket), `repeated_progress_dedupes_before_reaching_director` |
| Batch and context limits are obeyed | `director_batches_twenty_events_and_never_claims_two_active_turns`, `inline_batch_limit_defers_excess_and_stop_halts_future_turns` (20 envelopes or 32 KiB per director turn) |
| At the 1,000-envelope threshold new admissions stop while terminal events are kept | `full_inbox_rejects_routine_progress_but_keeps_terminal_result`, `full_director_inbox_holds_new_admissions_but_keeps_terminal_reports` (`director_inbox_full`) |
| Outstanding terminal reports are capped per attempt; an applied acknowledgement permits a correction | `terminal_inbox_bypass_is_bounded_per_attempt_and_replays_stay_idempotent` (16 outstanding, overflow unacknowledged so the sender retries, exact replays idempotent, a correction admitted after the director applies one) |

Boundary: the daemon's broker behaviour, measured on this machine with scripted senders. Whether a real harness retries an unacknowledged overflow, and a live director's context use, belong to the live qualification (SWARM-25).
