# Auto Allowance Fit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let Auto dispatch use only uncertainty-bounded, account- and task-scoped allowance estimates while preserving an honest unknown state when that evidence is unavailable.

**Architecture:** Keep subscription-window draw separate from token and thread-credit activity. The pure selector validates a candidate estimate against the exact work requirements, resolved model identity, account plan, and each live quota window. A capped local learning table holds only estimates produced from verified actual work or authoritative provider charges; dispatch reads it without starting a model turn and records the evidence in its replayable decision.

**Tech Stack:** Rust daemon, bundled SQLite, JSON protocol, controlled harness fixtures.

**Spec:** [Auto Mode RFC](../../rfcs/auto-mode.md), especially AUTO-AC-13, AUTO-AC-32, AUTO-AC-36, and the frozen [evaluation](../../verification/auto-mode/evaluation-v1.md).

## Global Constraints

- Do not infer subscription-window draw from tokens, public API dollar prices, or unvalidated thread credits.
- Keep accounts, plans, quota windows, model versions, and task requirements distinct; missing evidence stays unknown.
- Do not introduce a user-maintained routing file, paid calibration call, hosted telemetry service, or new credential path.
- Account switches and Clear usage-learning history invalidate learned estimates; execution and live quota history remain intact.
- Keep Auto disabled by default and manual routing unchanged.

## Review Focus

- A moving model alias with no resolved version must not reuse a past estimate; Task 1 tests this.
- An estimate for another plan, account generation, or quota window must not authorize fit; Tasks 1 and 2 test this.
- A task with different required tools or context must not inherit a cheaper task's draw; Task 1 tests this.
- A missing or locked learning database must keep dispatch available with `unknown` fit; Task 3 tests this.
- A reset or a second, nearly exhausted window must block a candidate even if another window has room; Task 3 tests this.

---

### Task 1: Comparable estimate contract

**Files:** Modify `daemon/src/auto_select.rs`; test in its existing unit module.

**Interfaces:** Extend `AllowanceEstimate` and `Route` with resolved version and task-scope fields; `assess_fit(snapshot, work, route, estimate, in_flight, now_ms) -> Fit` requires every scope to match before it may return `Fits` or `Unaffordable` from a numeric draw. Preserve old selector replay through versioned deserialization defaults.

- [x] Write unit tests for exact matching scope and unknown on missing/mismatched model version, task signature, plan, pool, and windows.
- [x] Run the targeted tests and observe failure.
- [x] Implement the scope validation without a numeric fallback or token conversion.
- [x] Run the targeted tests and verify pass; commit the contract with an honest ledger note.

### Task 2: Capped, invalidatable local evidence

**Files:** Modify `daemon/src/store.rs`, `daemon/src/auto_consumption.rs`, `daemon/src/auto_maintenance.rs`, and the production delta context in `daemon/src/server.rs`; test in store/consumption unit modules and the unattended protocol fixture.

**Interfaces:** A store method accepts a validated `AllowanceEstimate` with account generation and actual-work provenance, returns at most one current scoped estimate, and refuses mismatched generation. The trusted delta assessor is the only actual-window producer; all real currently unverified Codex readings still produce none.

- [x] Write failing tests for bounded sample insertion/read, account change, clear, expiry, and no promotion from unverified thread credits or deltas.
- [x] Run targeted tests and observe failure.
- [x] Add the learning table and methods, with a finite row/age cap and migration behavior.
- [x] Run targeted tests, ledger checks, and commit.

### Task 3: Dispatch and replay integration

**Files:** Modify `daemon/src/server.rs` and `daemon/tests/protocol.rs`; update `docs/verification/auto-mode/README.md`.

**Interfaces:** After discovery, dispatch reads the live normalized quota snapshot and same-generation local estimate for each candidate, calls `assess_fit`, and records source/version/uncertainty in the decision trace. Selector replay uses the saved scoped inputs and exact selector version. No estimate or failed learning read means `Fit::Unknown` and cold-start behavior.

- [ ] Write a controlled daemon test: an expensive candidate exceeds one applicable window while a suitable efficient candidate fits, and no provider ordering file is supplied.
- [ ] Write tests for unknown/locked learning, reset, account switch, and replay of the saved decision.
- [ ] Run targeted tests and observe failure.
- [ ] Connect the bounded estimator read and fit check; keep launch admission's existing unknown-draw pool claim until credible reservations are implemented.
- [ ] Run focused and complete serial tests, then the RFC ledger audit; commit and update criterion status without claiming live subscription-cost accuracy.

## Self-review

The plan covers the scope and consumption gates needed before learned fit can influence Auto dispatch. It deliberately leaves quality-aware efficiency ranking, measured reservations, and live provider proof to the remaining RFC criteria; this slice alone cannot close AUTO-AC-13, AUTO-AC-32, or AUTO-AC-36. The current provider reads lack validated model-version and external-work attribution, so real account estimates remain unknown until the required evidence exists.
