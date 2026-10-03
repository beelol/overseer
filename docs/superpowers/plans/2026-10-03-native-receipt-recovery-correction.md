# Native Receipt Recovery Correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans and test-driven-development in coordinator-assigned slots. Steps use checkbox syntax. This narrow correction follows reviewed Slice2 source and preserves every frozen behavioral baseline.

**Goal:** Give later orphan receipts bounded progress and recover written owner-denial policy after native retirement without reopening requests or sending answer bytes.

**Architecture:** Select private answer attempts using a durable rotating inspection ordinal, rather than selecting the first sixteen live request rows. Keep authoritative receipt disposition separate from the immutable first answer result. Historical receipt evidence may update once-only denial bookkeeping; only an exact current, live orphan claim may change public request lifecycle.

**Tech Stack:** Existing Rust, rusqlite/SQLite, private generation-bound shim status operation and fixture-only Unix RPC tests; no new dependencies or protocol fields.

**Spec:** `docs/overseer-rfc.md` AC274 and AC196; `docs/superpowers/plans/2026-10-03-native-pending-lifecycle.md` Slice2/recovery requirements; independent frozen5b review and frozenf643 fixture review.

## Global Constraints

- `5b96da3` runtime, `f643033` fixtures and earlier `9ba229f`/`6fddaef`/`c9933ae`/`a651699` baselines remain immutable. Compile-check exit0 is not behavioral qualification. Both recovery cases are UNRUN.
- No Cargo, helper, daemon, UI, paid/provider/browser/profile calls until coordinator allocation. Never use the active full-run integration target. Future focused execution uses nice20, jobs1/threads1 and an explicitly allocated absolute target.
- Never reconstruct, encode, resend or relaunch an answer during recovery. `request_reply_status` carries only saved generation/token/digest.
- Preserve terminal lifecycle, public projection/revision/cursor, authenticated device scope and immutable first results. A written receipt proves transport delivery, not operation completion or a fresh grant.
- This remains an unfinished Task2 branch. Full AC274 browser/installed/surface obligations and exact file-operation denial identity remain explicit gaps; grantRoot is not an exact denied file action.

## Review Focus

1. Sixteen active claims must not create status tombstones, but cannot starve a later orphan: actual held16+lost-ack17 fixture.
2. Persistently uncertain or gate-busy early attempts must advance inspection order too: source-level page-order test with seventeen unresolved private attempts; no network authority from this unit test.
3. Native resolution before collection must not hide an already-written denial: actual Resolved-before-query fixture preserves terminal public state and exactly one native response.
4. A superseded token must never overwrite a newer claim: recovery final checks compare current token/digest/revision before public changes; add focused saved-old-attempt/current-new-token unit assertion.
5. Revoked device or replaced generation must not acquire new answer authority from historical evidence: existing device/replacement fixtures remain required; historical query refuses a different generation/socket and never calls answer authority or emits response bytes.

---

## Files and interfaces

- Modify `daemon/src/pending_requests/answers.rs`: private migration, attempt receipt disposition, fair page selection and query-only reconciliation. Keep `reconcile(d: &Daemon, run_id: Option<&str>) -> Result<()>` unchanged.
- Modify `daemon/tests/pending_requests.rs`: strengthen frozenf643 burst assertion with exact frozen native response envelope and integer ID23; retain its seventeen requests and held-worker cleanup.
- Add private recovery unit tests inside `answers.rs` for selection rotation and superseded-token public-update eligibility. They prove storage decisions, not actual transport delivery.
- Evidence under `docs/verification/evidence/ac-274/task2-slice2`; no public protocol, generated types or frontend change.

## Storage and bounded progress

Add nullable private `native_answer_attempts.receipt_delivery` with only authoritative `written`/`not_written` values, and private `recovery_cursor INTEGER NOT NULL DEFAULT 0`. Add singleton `native_receipt_recovery_cursor(id=1, value)` for a durable inspection clock. Neither is exposed by state/events/results.

A short Store transaction selects at most16 unresolved attempts joined to their immutable request owner/display association and current same-generation run, filtered by the existing optional run selector. Order by `recovery_cursor, created_ms, delivery_token`. Increment the singleton clock with checked arithmetic and stamp every selected attempt before releasing Store. Every inspected candidate advances, including active, uncertain, disconnected or process-gate-busy cases. Concurrent readers reserve subsequent pages through the same Store transaction. Ordering is private and must not advance the public collection cursor. No caller-provided selector creates new cursor rows.

Authoritative direct written/not_written delivery and definite no-contact settlement set `receipt_delivery` transactionally; uncertain/error observations leave it NULL. Migration may backfill only a valid existing first-result `delivery: written|not_written`; all other old results remain unresolved. Keep canonical denial receipt insertion once-only. Do not clear or overwrite a final private delivery disposition.

## Historical evidence versus live eligibility

Each selected candidate is an **attempt**, identified by its own saved token, digest and generation; do not substitute the request row's latest token. Skip daemon-owned active tokens before any status query. Acquire the owning process gate with the existing bounded deadline, briefly snapshot the active registry, release its guard, then enter Store.

For the status query, require the stored attempt/request association, current owner process generation equal to the attempt generation, and the qualified LaunchFile/socket for that same generation. This historical transport check deliberately does not require an active native turn or nonterminal request lifecycle. It cannot query a replacement generation, invent a surviving shim when none exists, mint caller/device authority or change scope. Missing/changed process identity leaves uncertainty and advances the inspection cursor.

Drop Store before the bounded status control call. Verify exact ok/generation/token/digest and accept only authoritative written/not_written. Under the still-held process gate, revalidate the same attempt and qualified socket identity before committing private disposition and, for written, `record_written_denial` in one Store transaction. Revocation does not erase evidence of a prior qualified write; it still prohibits every new answer. No device gate is needed for a query that carries no answer data and performs no new protected operation.

Public lifecycle changes remain a separate decision: only the same current token/digest/revision, still claimed/uncertain, passing existing `live()` may be restored to pending or advanced to answered_awaiting_native. Historical terminal/superseded rows receive bookkeeping only: no `update`, no pending event, no changed revision/cursor, no reopening. Existing first result is never rewritten. A missing first result may be filled only by the existing eligible-current-orphan settlement path, not by historical retirement bookkeeping.

## Lock order

1. Page reservation: Store only; release before any process gate/registry/I/O.
2. Per candidate: bounded process gate → short active-registry snapshot, released → Store inspection, released → bounded status query → Store revalidation/transaction.
3. Active answer registration remains before claim publication. Registry guards never wait for Store/process; RAII drops only after process/Store guards have left scope.
4. Native application and generation changes already use the same process gate, preventing a validated surviving socket from being replaced during query/commit. Do not add task/device acquisitions under Store or process.

## Task1: Preserve and strengthen witnesses

- [ ] Commit the test-only exact ID23 envelope strengthening after frozenf643; do not alter its baseline.
- [ ] In an allocated slot, execute the two frozenf643 tests against unchanged5b runtime with fresh standalone/test artifacts. Expected failures are later-orphan lifecycle starvation and resolved-written denial count; classify any earlier setup failure separately.
- [ ] Preserve exact command/source/artifact/test-count logs before corrections are qualified. Source corrections may be authored while execution is held, but the unchanged baseline must run first.

## Task2: Fair private attempt paging

- [ ] Add private migration and `recovery_page(store: &Store, run_id: Option<&str>) -> Result<Vec<RecoveryAttempt>>`; `RecoveryAttempt` contains key/owner/token/digest/generation and saved actor, never response bytes.
- [ ] Add a private selection test: seventeen unresolved attempts, first page16, second page includes17 with earlier rows unchanged; all uncertain candidates stay eligible and rotation survives reopening the database.
- [ ] Stamp selected attempts transactionally before gate acquisition, including skipped/busy candidates; keep the sixteen-query ceiling and bounded per-candidate I/O.
- [ ] Preserve result immutability and store definite direct dispositions separately from recovery inspection order.

## Task3: Historical written policy with separate public settlement

- [ ] Replace `live()` as historical query eligibility with exact surviving transport-generation validation. Preserve `live()` for all new answers and eligible-current-orphan public updates.
- [ ] Commit only validated status metadata and once-only canonical denial bookkeeping for terminal/superseded attempts. No answer reconstruction and no terminal public mutation.
- [ ] Add superseded-token unit control and run the actual resolved-before-read case, lost-ack, active-claim and crash/tombstone controls in the allocated slot.
- [ ] Rerun exact burst/current-generation/device controls as scheduled; report all names/counts and remaining partial/dead-shim/file-identity gaps honestly.
- [ ] Request independent source/evidence review, push draft recovery checkpoints, and leave full/browser/rendered qualification to coordinated gates.

## Self-review and remaining scope

This correction addresses the two concrete reviewed recovery defects only. Fair attempt inspection does not promise a receipt when the qualified shim no longer survives. Historical written policy does not restore native turn eligibility, device pairing, grants, Voice readbacks or proposal confirmations. Partial-write, receipt-loss/old-generation cleanup, richer file identity, three-surface structured rendering and live browser qualification remain required by the broader approved plan and are not declared complete here.
