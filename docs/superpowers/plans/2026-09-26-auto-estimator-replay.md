# Auto Estimator Replay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make new Auto decisions replay the allowance-fit calculation from saved, content-free inputs, rather than trusting a saved fit label.

**Architecture:** Split `auto_fit` into bounded collection of normalized evidence and a pure evaluator. Record the evaluator's exact work/route/clock/quota/estimate inputs in a new v7 decision trace. Replay v7 by evaluating those saved inputs and selecting again; retain v1–v6 behavior and mark their replay scope honestly.

**Tech Stack:** Rust daemon, serde JSON protocol, local SQLite, controlled harness fixtures.

**Spec:** [Auto Mode RFC](../../rfcs/auto-mode.md), especially AUTO-AC-15; [verification ledger](../../verification/auto-mode/README.md).

## Global Constraints

- No raw prompts, titles, credentials, provider responses, or transcript text in estimator inputs.
- No network call, model turn, telemetry deployment, or paid calibration for replay.
- A missing, stale, oversized, or unparseable evidence set leaves numeric fit unknown.
- Older selector versions retain their recorded replay semantics.
- Keep Auto disabled by default and manual routing unchanged.

## Review Focus

- A saved `fit` value changed without its evidence must fail full replay, not silently pass.
- A quota window changed from 95% to 99% used must change the recomputed numeric fit.
- An unknown route with no prediction must replay as unknown without fabricating an estimate.
- A trace exceeding the size bound must not store partial evidence while retaining a numeric fit.
- Historical v6 traces must still replay as selector-only, with no claim that their fit was recomputed.

---

### Task 1: Pure, bounded fit evidence

**Files:** Modify `daemon/src/auto_fit.rs`; test its unit module.

**Interfaces:** `FitEvidenceInput` is a serde-serializable, content-free enum for unavailable evidence or normalized quota snapshots plus one scoped estimate. `evaluate_fit(work: &WorkUnit, route: &Route, input: &FitEvidenceInput, now_ms: i64) -> FitEvidenceResult` returns fit, reason, and public provenance. Collection reads the store and returns one input per route. A bounded trace budget downgrades all numeric fits to unknown before selection when complete inputs cannot be retained.

- [ ] Add a red test: a controlled predictive envelope and two same-account meters select the suitable route; serialized inputs reproduce both fits, and changing one saved meter changes the recomputed result.
- [ ] Add a red test: missing prediction, learning lock, and oversized evidence remain unknown with no partial numeric trace.
- [ ] Extract the pure evaluator and bounded collector without changing the source-of-truth scope gates or unknown-draw admission.
- [ ] Run focused tests and commit the self-contained evidence contract.

### Task 2: Versioned full replay

**Files:** Modify `daemon/src/server.rs`, `daemon/tests/protocol.rs`, and `docs/verification/auto-mode/README.md`.

**Interfaces:** New dispatches record selector version `multi-harness-preflight-v7`, `estimator.version`, `estimator.now_ms`, and the complete bounded inputs. `auto.decision.replay` recomputes each v7 fit and then runs the v7 selector; it reports `replay_scope: "selector_and_estimator"` and whether both match the recorded decision. v1–v6 remain `selector_only`.

- [ ] Add a red daemon protocol test that a v7 cold-start decision replays with estimator recomputed and no prompt or secret sentinel in the trace.
- [ ] Add a red tamper test: altered saved fit or normalized quota evidence makes v7 replay report a mismatch; v6 still reports selector-only.
- [ ] Wire v7 trace and replay; reject missing or oversized replay inputs rather than trusting stored fit.
- [ ] Run focused and full serial workspace tests, four ledger mutation tests, criterion audit, and diff check; update AUTO-AC-15 evidence without promoting it until the full Verify clause is proved, then commit.

## Self-review

This plan covers estimator replay only. It does not make a completed-work sample predictive, resolve model aliases, validate live provider charges, or implement routing inference. A v7 trace may record the absence of those features; AC-13, 32, 34, and 36 remain independent. Task 1 must cap serialized evidence before dispatch, so Task 2 cannot claim full replay from a truncated trace.
