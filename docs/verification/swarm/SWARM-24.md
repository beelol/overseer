# SWARM-24 — shared Auto Mode boundary

Status: partial. Support level: proposed contract plus Swarm fixture admission; no merged Auto producer or shared cross-mode account reservation transaction.

Input: compare this branch's `swarm.policy.preview` and `swarm.admit` with the separate `codex/automode-rfc` branch at `11b4cf9`, including its quota/route selector and durable `auto.dispatch` launch intent. Review current daemon `account.usage`, `account.list`, `harness.list`, and `profile.status` APIs. The contract request includes run/parent/job identity, attempt budget, native-unit upper estimates, and remaining category allocation; the transaction must apply the tighter account and category limits across every binding window.

Expected: ordinary, Auto and Swarm callers share one account-pool reservation and workspace/agent admission transaction. Stale observations and changed account identity force revalidation; a confirmed pre-effect failure can use remaining attempts, while an uncertain effect retains its commitment and cannot reroute. Contract proofs are shared with AUTO-AC-04/17/19/20/24, not separately certified.

Actual: Swarm fixtures revalidate injected snapshots, reserve their own fixture pool rows and share the app-wide agent-slot lock with ordinary starts. Auto's separate branch has structured quota observations, scoped route selection and selected launch intents, but its launch-boundary design explicitly leaves atomic allowance commitment and complete crash reconciliation open. Existing account APIs are observations, not the shared reservation ledger. The contract document records this boundary and the common acceptance-proof map. No adapter or shared cross-mode account transaction exists yet.

Evidence: `docs/rfcs/swarm-auto-contract.md`, `docs/rfcs/swarm-mode.md`, `daemon/tests/swarm_admission.rs`, and read-only comparison to `codex/automode-rfc:docs/verification/auto-mode/launch-boundary-design.md` at `11b4cf9`.

Remaining: merge or otherwise integrate the Auto producer behind a versioned adapter; implement the single shared admission authority and route-to-launch revalidation; run concurrent ordinary/Auto/Swarm pool and writer tests, changed identity, stale-known/unknown and crash/reconnect tests. Keep SWARM-24 unchecked.
