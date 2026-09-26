# Unverified Swarm acceptance criteria — dependency audit

Initially inspected at code revision `ffa08ba`; updated after automatic terminal reconciliation at `c37ca47`. This is a gap audit, not verification evidence. The 23 criteria still marked `unverified` in `coverage.json` each have a specific reason. None is checked in the RFC, and S0–S5 remain unverified.

The dependency clusters are:

1. Normal launch and Auto Mode integration: SWARM-01/02/05/13/15/16/17/25/26/28/51/59/63. Target and quota fixtures cannot establish live account availability, worker launch, descendant control, liveness, native usage or comparative benefit. The separate Auto Mode branch has only an RFC at this checkpoint; the contract in `docs/rfcs/swarm-auto-contract.md` must be agreed before sharing admission state.
2. Shared workspace and side effects: SWARM-18/52/57/58. Swarm assignments do not yet select isolated worktrees or enforce read-only source/service permissions, destination ACLs, or side-effect reconciliation.
3. Director context and multi-category scheduling: SWARM-34/36. A bounded message batch is implemented, but not a bounded worker brief/context request path or fair central admission queue.
4. Product presentation and truthful outcomes: SWARM-23/37/55. The extension has no Swarm view, and final coverage does not yet classify negative results, environment failures and confirmed defects.
5. End-to-end scenarios: SWARM-64. A scripted Swarm worker now has partial SWARM-31 evidence, including once-only terminal delivery and deadline interruption, but complete versioned S0–S5 traces and live harness lifecycle qualification have not been replayed.

Each `blocker` field in `coverage.json` names the missing behavior for the 23 criteria and six scenarios. A partial criterion stays partial where fixture evidence exists; an unverified criterion remains unverified even if adjacent code has tests. No lack of live authorization was turned into a passing fixture claim.
