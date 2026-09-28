# Prepared goal: implement category-directed Swarm mode

Status: not activated. This file is a reusable instruction for a future implementation session.
Scope: [RFC](swarm-mode.md), SWARM-01–64, and [scenarios S0–S5](swarm-mode-scenarios.md).

## Goal text

Implement the approved category-directed Swarm RFC in an isolated Overseer checkout and
verify every required acceptance criterion, SWARM-01–64. Deliver the director/worker
communication loop, automatic defaults, adaptive admission, shared account budgeting,
scope/overlap coordination, durable recovery, and truthful evidence-based completion.
Use scenarios S0–S5 as executable reference workflows, including their injected failures.

Coordinate the automode contract and single shared reservation authority before integration.
Do not replace the other agent's work, create competing telemetry collectors, or broaden
this goal into VS Code production-readiness, a full Agetor fork, or unrelated product work.
Port selected Agetor logic/tests only when justified, retaining revision and license notices.

Before implementation, snapshot the approved RFC/policy revision, support matrix and goal
execution authority. The user must establish the implementation session's time/token allowance
and any live-test/publication authority when activating the goal; never infer those from the
product's 60-minute default or a previous agent's session. Produce the implementation plan
and required integration agreement before starting code changes.

Work through criteria in dependency order:

1. Shared target/admission contract, policy defaults, durable state and message broker.
2. Director/worker communication, acknowledgements, revisions, ownership and scope checks.
3. Scheduling/account budgets, recovery/cancellation, artifact verification and integration.
4. Normal launch/status controls and large-swarm observability.
5. S0–S5 fault replays, Agetor parity ledger, scale checks and supported live paths.

For each iteration:

- Select the next unmet criterion whose dependencies are available.
- Reproduce its missing or incorrect behavior using a focused deterministic fixture/test.
- Implement a bounded fix and run the relevant checks, including regressions for affected
  already-verified behavior. Do not repeatedly rerun unrelated expensive suites.
- Record revision, inputs, commands/steps, expected/actual outcomes and evidence paths in
  `docs/verification/swarm/SWARM-XX.md`. Check its authoritative RFC box only when the whole
  criterion is verified. Any change invalidating earlier evidence reopens the affected boxes.
- Continue until all required criteria are verified or the session reaches an actual stop
  condition. Track blockers separately and continue independent work where possible.

Use local/scripted harnesses for message loss, restart, quota, concurrency and 32-worker tests.
Only perform separately authorized tiny live account tests; no repeated paid benchmarks,
production mutation, purchases, login changes, or automatic merge. Fixture-only evidence does
not qualify a live harness. Unsupported communication/control paths stay explicitly unverified.

Never weaken or delete criteria to finish, fabricate successful evidence, or count blocked
coverage as passing. Do not endlessly retry unchanged failures. If all remaining work requires
missing access, an external dependency or a product decision, preserve progress and report that
specific blocker; observe the active goal system's rules when setting blocked status. If the
authorized session budget expires or the user stops, checkpoint and leave the goal incomplete.

Completion requires all SWARM-01–64 boxes verified, all required S0–S5 scenarios passed with
reproducible evidence, consistent support/limitation documentation, and no known regression
invalidating a checked criterion. Report final revision, evidence index, supported combinations
and any residual limitations. A partial milestone is progress, not goal completion.

## Activation boundary

Creating this document is planning work. No goal, scheduled task, agent swarm, live test or
implementation has been started. Once the RFC and criteria are accepted, activate the goal
with the user's implementation-session limits and publication preferences.
