# SWARM-60 — scoped peer communication

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`); see the last section. Earlier status: partial at `9cbe10c`.

Input: the same Atlas-style D1 fixture reports one worker discovery and directs a short advisory to two of three other registered workers. The unrelated membership worker is excluded.

Expected: the director mediates peer sharing; recipient-specific delivery and application are durable, while worker text cannot issue director directives.

Actual: the broker stores J2's discovery in the director inbox, accepts only director-origin advisories for J1/J4, preserves delivered/applied phases, and rejects a worker-origin advisory. J3 receives no advisory. The focused broker suite passed 9 tests.

Evidence: `daemon/tests/swarm_broker.rs`, `daemon/src/swarm/broker.rs`.

Remaining: no live harness delivery, automatic relevance choice, destination artifact permissions, or cross-category access policy is qualified. This is a scripted daemon contract, so the criterion remains unchecked.

Worker-inbox isolation at `7ee7d0c`: the fixture test first showed that an attempt ID alone could read a targeted redirect, even though applying it required the worker token. `swarm.messages` now verifies the recipient's attempt token before returning worker directives. Missing and wrong tokens are rejected; the recipient still reads, delivers and applies its own directive. Scripted Atlas J4 now includes its private token when polling, and the dispatch fixture checks the applied record without exposing that token in a launch response. The full daemon test suite passed serially, including 18 broker tests; all 16 opt-in Atlas PostgreSQL tests passed. This is a daemon/scripted-worker boundary test, not proof of provider-specific credential isolation or live destination permissions.

## Status on 2026-09-28 (`claude/auto-swarm`)

Progress this session (`866b0a58`): Overseer's message, redirect, hold, stop, guardrail, area, report, cadence and a holding watch aimed at a Swarm worker are refused before any proposal, at every level, and the refusal names the director; a message or redirect to the director enters the swarm's durable director inbox as an `advisory` from `overseer` with its proposal and approver (`overseer_actions_aimed_at_a_swarm_worker_are_refused_and_offered_to_the_director`, red before the fix). A read-only watch of a worker is allowed.

Still partial. Not built: ordinary agents' reports, asks and claims (Gate S's `agent_messages` and `areas`) and Swarm's broker are still two stores, so "one durable broker" and one claim ledger (also SWARM-44) are not true yet; Gate S's RFC says whichever lands second adopts the first one's tables. A watcher's finding reaching the director as an advisory goes through Overseer's message to the director, which is now an advisory, but no test joins a watcher to it.

## Verified at fixture scope (2026-09-28, second session)

Built: one durable broker ledger (`daemon/src/broker.rs`, table `broker_envelopes`). Swarm's broker keeps its envelopes in `swarm_messages` and Gate S's channel keeps an ordinary agent's report, ask and claim in `agent_messages`; every envelope of either is also one row of the one ledger, with one stable id (`swarm/<run>/<message>` or `agent/<message>`), received once however often it is repeated, and separate `delivered` and `applied` states (plus `refused` for a claim the claim ledger refused). Swarm envelopes enter and change phase by triggers on its own table, in the same transaction; Gate S's enter by trigger and are marked delivered when they reach Overseer's conversation and applied when an ask is answered, a claim is written or a report is taken into Overseer's next turn. Gate S's channel now also bounds a body at 32 KiB like a Swarm envelope. `broker.envelopes` reads the ledger. Existing envelopes are copied in once on upgrade. Claims share one claim ledger too ([SWARM-44](SWARM-44.md)).

A read-only watcher's finding about a Swarm worker now goes to Overseer (its card, as before) and on to the director's inbox as an advisory from `overseer` whose source is the watcher, naming the finding and the job it was about, with no job or attempt on the envelope; Overseer's check-in goes to the director, not the worker. Before this the finding queued a check-in on the worker and nothing reached the director.

| Clause | Test |
| --- | --- |
| Peer evidence and questions on one durable broker with stable ids, durable receipt, separate delivery and application | `report_is_durable_before_ack_and_replay_is_idempotent` (one envelope, `swarm/<run>/discovery-1`, across a SIGKILL restart and a replay); `directive_delivery_and_application_are_distinct` (the ledger row moves queued → delivered → applied with both times; a replay adds none) (`swarm_broker.rs`) |
| …and ordinary agents' reports, asks and claims on the same broker | `ordinary_agents_share_the_swarm_broker_and_a_watchers_finding_reaches_the_director` (`swarm_gate_s.rs`, new, red first: a 33 KiB report was `Recorded.`): report (sent twice), ask and claim are three envelopes `agent/<id>` from the agent's run to `overseer`; the report and ask are delivered, the claim applied; answering the ask makes it applied; a body over 32 KiB is refused |
| Peer text cannot reassign ownership or admit jobs | the new test (an agent's report telling the director to reassign J2 and admit a job changes no job); worker-origin advisory refused (`discovery_can_be_routed_to_only_relevant_peers_with_applied_receipts`); `atlas_s1_faults_quarantine_stale_and_missing_evidence` (a "director command" in source text stays data, opt-in) |
| A shared-helper discovery reaches affected peers only, without unrelated transcripts or category-restricted artifacts | `discovery_can_be_routed_to_only_relevant_peers_with_applied_receipts` (J1/J4 get D1, J3 does not); `categories_share_accounts_but_not_scopes_ledgers_budgets_or_evidence` (`swarm_scheduler.rs`: another category's worker cannot read A's evidence); `hundred_job_summary_and_scoped_large_artifact_context` (`swarm_context.rs`: briefs carry artifact references, not transcripts; destinations without a grant cannot read) |
| Overseer's message, redirect or hold at a worker refused and offered as a sourced director advisory | `overseer_actions_aimed_at_a_swarm_worker_are_refused_and_offered_to_the_director` (`866b0a58`) |
| A read-only watcher's finding follows the same advisory path | the new test: Overseer proposes a read-only watch of the worker with an idle agent as watcher; its `concern` finding is Overseer's card and one advisory in the director's inbox (source `watcher`, the finding, no job or attempt, in the broker ledger); no check-in, message or hold reaches the worker, which keeps running; jobs are unchanged |

Runs on 2026-09-28, one file at a time with `--test-threads=1`: `swarm_gate_s` 4 passed; `swarm_broker` 24; unit `store` 45 and `swarm::` 14; `overseer` (Gate S) 24.

Boundary: scripted director (S0 fixture) and generic ordinary agents; no live harness delivery.
