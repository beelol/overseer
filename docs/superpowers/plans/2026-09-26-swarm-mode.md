# Category-directed Swarm implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement [the Swarm RFC](../../rfcs/swarm-mode.md) and verify SWARM-01–64 plus S0–S5 in the isolated `codex/swarm-mode` worktree.

**Architecture:** The existing Rust daemon remains the sole state and admission authority. A new `swarm` module stores category runs, logical jobs, attempts, message envelopes, artifact references, claims, quota pools and reservations in the same SQLite database as normal Overseer runs. Director and worker turns use existing harness adapters when their control/communication capabilities are proved; automode supplies targets and telemetry through a versioned contract. The VS Code extension shows aggregate state and controls without loading every transcript.

**Tech Stack:** Rust 2021, `rusqlite`, `tokio`, `serde_json`, existing Unix-socket JSON protocol; VS Code extension JavaScript.

**Spec:** `docs/rfcs/swarm-mode.md`; `docs/rfcs/swarm-mode-scenarios.md`; contract at `docs/rfcs/swarm-auto-contract.md`; reusable goal at `docs/rfcs/swarm-mode-goal.md`.

## Global constraints

- User explicitly requested an isolated worktree and PR, no interruptions for questions, and honest partial evidence. Preserve other agents' work in the original checkout.
- One category run has one director generation; only it may change assignments. Every daemon mutation validates generation and revision.
- Default ceilings: 8 workers/run, 9 executing agents globally; growth at most 4 per 5 seconds; 60-minute run deadline; 20% minimum finishing reserve; at most 2 attempts/job.
- Saved category > application > built-in defaults; explicit run override wins, permission restrictions always bind.
- No automatic fan-out on unknown/stale/incompatible quota or uncalibrated estimates; no invented token/percentage conversion.
- Broker persists before acknowledging, uses stable IDs and idempotent transitions. Receipt and application acknowledgements differ. No worker text is authoritative director input.
- No unsupported claims: fixtures do not establish live harness support. Do not check an AC until every clause has evidence. No live paid loops, account/login changes, purchases or automatic merge.
- Existing daemon APIs and non-swarm behavior remain compatible; the `swarm` namespace owns new protocol methods.

## File and interface map

| File | Responsibility |
| --- | --- |
| `daemon/src/swarm/schema.rs` | SQLite schema and typed row mapping; migration independent of base tables |
| `daemon/src/swarm/policy.rs` | Defaults, effective limits, pool arithmetic, admission reasons |
| `daemon/src/swarm/broker.rs` | Durable message envelope validation, dedupe, receipt/application status, bounded inbox |
| `daemon/src/swarm/plan.rs` | Dependency DAG, revisions, claims, overlap and acceptance transitions |
| `daemon/src/swarm/scheduler.rs` | Director lease, ready queue, slots, waves, reservations and fair category admission |
| `daemon/src/swarm/runtime.rs` | Harness delivery, launch/checkpoint/recovery, cancellation and status reconciliation |
| `daemon/src/swarm/mod.rs` | Public daemon-facing operations and summaries; narrow connection to existing `Daemon` |
| `daemon/src/store.rs` | Calls schema migration during `Store::open`; no base-table rewrite |
| `daemon/src/server.rs` | Versioned `swarm.*` dispatch methods |
| `extension/src/extension.js`, `views.js`, `extension/package.json` | Start/control swarm and readable aggregate tree |
| `daemon/tests/swarm_*.rs`, `extension/test/*` | Deterministic policy, broker, scenario and UI tests |
| `docs/verification/swarm/*` | Per-AC result/evidence ledger and scenario traces |

Only add a new adapter surface to `adapters.rs` when the existing one cannot deliver a bounded, acknowledged directive. Feature detection remains explicit per harness. Automode's collector remains separate; the new contract consumes its snapshots when available and can consume injected deterministic snapshots for tests.

## Review focus

1. Duplicate receipt after persistence but before acknowledgement must neither reaccept a result nor double reserve capacity: tested in Task 3.
2. Account identity aliases across harnesses must map to one quota pool: tested in Task 5.
3. A director lease expires while its process still runs; replacement cannot cause two command sources or release live usage: tested in Task 6.
4. A worker receives a revision while inside a long tool call; delivery is not application and dependent work stays held: tested in Task 3 and Task 6.
5. Results arrive during Stop, permission revocation or full storage; no new admission, no lost acknowledged artifact: tested in Tasks 3, 6 and 9.

---

### Task 1: Freeze inputs and set evidence discipline

**Files:** `docs/rfcs/swarm-mode*.md`, `docs/verification/swarm/README.md`, `docs/verification/swarm/coverage.json`

**Interfaces:** Coverage record `{id,status,evidence,revision,blocker}`; one row per SWARM-01–64, plus scenario records S0–S5. This document does not alter boxes by itself.

- [x] Record base SHA, the three RFC file hashes, baseline test results and current automode contract evidence. Compare any concurrent automode branch without merging its work by assumption.
- [x] Write a coverage ledger that marks all 64 criteria unverified initially and tracks `implemented / unverified`, `partial`, `blocked`, `failed`, `verified` without conflating them.
- [x] Check every local RFC link and number, then commit the copied RFCs, ledger and this plan on the isolated branch.

### Task 2: Durable category, job and attempt state

**Files:** `daemon/src/swarm/schema.rs`, `daemon/src/swarm/plan.rs`, `daemon/src/swarm/mod.rs`, `daemon/src/store.rs`, `daemon/src/main.rs`, `daemon/src/server.rs`, `daemon/tests/swarm_state.rs`

**Interfaces:** `swarm.create({category,objective,allowed_targets,policy}) -> SwarmSummary`; `swarm.get({id}) -> SwarmSummary`; `swarm.plan({id,generation,revision,jobs}) -> PlanRevision`; `swarm.jobs({id,cursor,limit}) -> Page`.

- [x] Write failing tests for category uniqueness, one active run, 100-job pagination, invalid cycles/dependencies, revision CAS, and unchanged non-swarm state.
- [x] Run `cargo test --offline --test swarm_state` and confirm those named cases fail.
- [x] Implement schema/mapping/validation and protocol methods; keep object identity separate from attempts and process IDs.
- [x] Run focused test and `cargo test --offline`; record expected/actual state, then commit.

### Task 3: Durable broker and applied directives

**Files:** `daemon/src/swarm/broker.rs`, `daemon/src/swarm/schema.rs`, `daemon/src/server.rs`, `daemon/tests/swarm_broker.rs`

**Interfaces:** `swarm.report({run_id,job_id,attempt_id,message_id,type,revision,payload}) -> Receipt`; `swarm.messages({run_id,recipient,cursor}) -> Page`; `swarm.ack({message_id,recipient,phase,revision}) -> Ack`; `swarm.direct({run_id,generation,job_id,type,revision,payload}) -> Receipt`.

- [ ] Write failing tests for pre-ack durability, replay idempotence, role/permission validation, obsolete revisions, delivered versus applied, bounded payloads/inbox and 2,000 repeated progress messages.
- [ ] Run focused test and confirm failures.
- [ ] Implement persistence-first broker and make `stop`/revocation dispatch holds independent of any model turn. Reject role claims embedded in text.
- [ ] Run focused and existing tests; record evidence for applicable SWARM-41–43/49/50/60/61, then commit.

### Task 4: Plan, claims and result acceptance

**Files:** `daemon/src/swarm/plan.rs`, `daemon/src/swarm/schema.rs`, `daemon/src/server.rs`, `daemon/tests/swarm_plan.rs`

**Interfaces:** `swarm.claim({run_id,generation,job_id,resource,mode})`; `swarm.decide({run_id,generation,job_id,revision,decision,evidence})`; `swarm.revise({run_id,generation,expected_revision,changes})`.

- [ ] Write failing tests for shared read versus exclusive write, duplicate hypotheses, explicit independent reproduction, DAG validity, stale contracts, artifact provenance, contradictory findings and rejection not unlocking dependents.
- [ ] Run focused red tests; implement claim and acceptance state machine with revision-checked decisions.
- [ ] Run focused and existing tests; record evidence for SWARM-03/18/19/21/35/44–48/53–55, then commit.

### Task 5: One admission authority and default budget policy

**Files:** `daemon/src/swarm/policy.rs`, `daemon/src/swarm/scheduler.rs`, `daemon/src/swarm/schema.rs`, `daemon/src/server.rs`, `daemon/tests/swarm_policy.rs`

**Interfaces:** `TargetSnapshot {target_id,account_id,pool_ids,capabilities,health,observed_ms,expires_ms,windows}` from automode or a fixture; `swarm.admit({run_id,job_id,snapshot_version}) -> Admission|Reason` uses a SQLite transaction shared with ordinary launches.

- [ ] Write failing tests for S0 defaults, 10% allocation, 20% reserve, binding windows, independent/shared profiles, unknown/stale quota, unlike units, incompatible models, concurrent swarm/non-swarm admission and 32-worker explicit override.
- [ ] Confirm red; implement one shared reservation ledger and fail-closed decision reasons. Make normal `task.create` participate in the same concurrency/account authority where identity is known.
- [ ] Run focused and base protocol tests; record evidence for SWARM-01/02/04–14/24/28/39/40/56, then commit.

### Task 6: Director and worker runtime

**Files:** `daemon/src/swarm/runtime.rs`, `daemon/src/swarm/scheduler.rs`, `daemon/src/daemon.rs`, `daemon/src/server.rs`, `daemon/tests/swarm_runtime.rs`

**Interfaces:** `swarm.start`, `swarm.tick`, `swarm.pause`, `swarm.resume`, `swarm.stop`, `swarm.reconcile`; runtime persists launch intent and generation before invoking an adapter, then confirms process identity before releasing it.

- [ ] Write failing tests using scripted harnesses for director election/replacement, saved plan, first wave/queue, direct mid-run discovery, checkpoint delivery, lost launch acknowledgement, late result, Stop race, native-child control and run deadline.
- [ ] Confirm red; implement bounded director wakeups, eligible worker launches, drain and reconciliation. For harnesses without verified directive/child control, report ineligible instead of silently launching unmanaged workers.
- [ ] Run focused and base protocol tests; record evidence for SWARM-15–17/20/22/27/29–33/41/43/51/59/61/62, then commit.

### Task 7: Isolation and integration

**Files:** `daemon/src/swarm/runtime.rs`, `daemon/src/swarm/plan.rs`, `daemon/src/git.rs`, `daemon/tests/swarm_integration.rs`

**Interfaces:** Workers hold isolated worktrees and scoped resource claims. `swarm.integrate({run_id,generation,job_id,artifact_id,base_revision})` verifies the submitted base, applies only accepted output to the run's integration workspace, and stores conflict artifacts without data loss.

- [ ] Write failing tests for independent writers, dirty current checkout, read-only audits, overlapping patches, stale source, failing combined test, uncertain side effect and no automatic publishing.
- [ ] Confirm red; implement integration/verification states and preserve partial outcomes.
- [ ] Run focused and base tests; record evidence for SWARM-16/18/19/35/52–55/58, then commit.

### Task 8: Simple VS Code launch and scalable status

**Files:** `extension/package.json`, `extension/src/extension.js`, `extension/src/views.js`, `extension/src/daemon-client.js`, `extension/test/swarm.js`

**Interfaces:** `Overseer: Start Swarm`, `Pause Swarm`, `Resume Swarm`, `Stop Swarm`; category/objective prompt plus one-time account selection; state summaries/page requests. Existing task commands stay unchanged.

- [ ] Write focused UI/client fixture tests for ordinary S0 launch, 100 jobs/32 active summaries, status filters, changed preset/permission, displayed uncertainty and Stop responsiveness.
- [ ] Confirm red; implement controls and lazy job detail. Keep editor behavior out of normal swarm execution.
- [ ] Run `npm run check`, focused test, package smoke and relevant UI tests; record SWARM-23/37/39/63, then commit.

### Task 9: Scenarios and fault replay

**Files:** `fixtures/swarm/*`, `daemon/tests/swarm_scenarios.rs`, `docs/verification/swarm/*`

**Interfaces:** Each S0–S5 replay loads a versioned fixture and emits dispatch/message/artifact traces for assertions; no paid model needed.

- [ ] Add Atlas route/DB fixtures (S0/S1), LedgerPay event/queue fixture (S2), Catalog 24-module fixture (S3), Dispatch sanitized logs (S4), and S5 fault switches.
- [ ] Run red scenario tests; implement missing cross-component behavior, fixing the owning unit rather than weakening the fixture.
- [ ] Run all deterministic replays, 100-job/32-worker load, fault/restart and old protocol tests. Record actual traces and mark only fully proven ACs verified, then commit.

### Task 10: Live compatibility, evidence audit and PR

**Files:** `docs/compatibility.md`, `docs/verification/swarm/*`, `docs/rfcs/swarm-mode.md`, PR description.

**Interfaces:** One evidence record per AC, support matrix by harness/account/platform, and a final AC audit table with proof or exact remaining gap.

- [ ] Qualify each claimed harness communication/control path with separately authorized tiny live runs. Use fixture labels where access is missing; no paid load loop.
- [ ] Compare the pinned Agetor surfaces and port only justified behavior with notices. Write SWARM-38 ledger.
- [ ] Re-run affected tests, inspect evidence for all 64 criteria and S0–S5, update authoritative checkboxes only where fully proven, and report partial/blocked items honestly.
- [ ] Commit, push `codex/swarm-mode`, create a **draft PR** with concise behavior and actual verification. Attach the PR to this task. Do not merge automatically.

## Execution and evidence

The user selected implementation in this session and asked not to stop for routine choices.
Execute natively in dependency order. At each milestone, state what is proven and what remains;
continue independent work after a blocker. If baseline or environment tests fail, inspect the
failure before changing product code. A passing fixture is no substitute for live support.

Completion is a strict audit of every SWARM-01–64 row and S0–S5. The PR can remain draft with
explicit missing evidence. Do not mark the goal complete until all required evidence exists.
