# Pending answer and receipt qualification manifest

Status: source-authored, UNCOMPILED/UNRUN recovery correction; no Cargo allocation. Root reviewed5b compile-check only. This manifest does not convert prior setup/missing-API failures into behavioral results. Root owns full/UI gates; no ready/merge claim.

## Frozen witnesses and mechanical prerequisites

| Witness | Frozen baseline | First required boundary |
|---|---|---|
| Earlier twelve Slice2 cases | 7cc7df9/89710d | Eleven reached missing method/classification/private receipt boundaries; veto had collection SETUP failure. Preserve original log. |
| Native tool inherited Unix authority and delayed-request tombstone | 9ba229f | Actual authenticated native tool cannot inherit LocalOwner; delayed original token cannot write after authoritative absent status. |
| Same-generation actual turn rollover/completion | 6fddaef | A claimed reply cannot send after accepted B/completion; healthy current native frame remains answerable. |
| Captured Needs and written denial | c9933ae | Captured request/revision A cannot become B; qualified written denial preserves incumbent no-workaround policy. |
| Handle helper second boundary | 8f2ef83 | Changed identity after preliminary check returns stale preparation, not old text/proposal:null. |
| Active claim, lost acknowledgement and daemon crash | a651699 | Active owner is not tombstoned; orphan status query never sends; missing receipt tombstone after real crash requires fresh owner answer. |
| Sixteen active claims plus later orphan; resolution before query | f643033, strengthened test-only fff9216 | Later receipt progresses without releasing earlier claims; terminal request remains byte-for-byte public stable while written denial is recorded once. Runtime at fff9216 is unchanged5b. |

The fake Claude --version correction0439 and literal child-turn notification correction2c8e511 are mechanical SETUP fixes, not product RED. Record them explicitly if needed by an earlier baseline; do not import later authority/context/answer/recovery runtime fixes into a baseline. Before allocating a command, resolve the exact baseline tree and inspect any mechanical test-only patch. Baseline tests may stop at an earlier boundary; report only the reached assertion. For the two current recovery witnesses use fff9216 directly: it already contains all prerequisites and retains unchanged5b runtime.

## Artifact and command discipline

Use only a coordinator-allocated retired target; never `/private/tmp/overseer-closeout-integration-target`. Proposed target is `/private/tmp/overseer-closeout-verify-target`, subject to explicit allocation. Commands below are a manifest, NOT executed. One Cargo process, `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `nice -n 20`.

Before changing the clone source used by a shared cache: clean **only overseerd package artifacts** in the exclusively allocated target, build standalone `cargo build -p overseerd --bin overseerd`, and build the exact test with `cargo test -p overseerd --test pending_requests --no-run --message-format=json`. Record clean HEAD, cwd, actual priority, target path, complete commands/exit, fresh:false artifacts, `.d` paths pointing to this source, standalone daemon and test executable SHA256/mtime and test listing. Metadata-only `cargo check`, JSON source paths alone, stale/zero matched tests or a fixture using another clone's daemon cannot establish runtime freshness.

The daemon fixture helper resolves `CARGO_BIN_EXE_overseerd`; verify it is the freshly built standalone owned target executable. The same binary provides its shim route. Native mock programs are fixture paths in this checkout; --version may run but no installed harness/provider turns. These tests own temporary HOME/data/process/socket directories; no owner daemon, native speech or browser is needed except the separately explicit simulated Voice fixture below.

## Execution order after allocation

1. Execute current unchanged5b baseline at fff9216, exactly one test per command:
   - `cargo test -p overseerd --test pending_requests slice2::receipt_recovery_progresses_past_sixteen_active_claims_without_tombstoning_them -- --exact --nocapture`
   - `cargo test -p overseerd --test pending_requests slice2::resolved_native_decline_recovers_written_policy_without_reopening_request -- --exact --nocapture`
   Each must match1 test. Expected grounded failures are the later orphan lifecycle and missing historical denied row. Earlier helper/timeout/schema failure is SETUP and must not count as either intended RED.
2. Build the reviewed correction with the same artifact discipline; prove basic actual typed answers first:
   - `slice2::frozen_typed_family_answers_reach_exact_native_transport` (one matrix test; report actual native family/vector captures reached, not just1 test)
   - `slice2::persistent_native_veto_refuses_stale_always_but_allows_once` (corrected setup, independently requalify)
   - `slice2::competing_answer_claims_emit_one_exact_native_response`
   - `slice2::wrong_family_revision_and_caller_authority_emit_no_response`
3. Execute preserved authority/receipt witnesses and matching correction controls:
   - `slice2::native_token_tool_cannot_inherit_local_owner_transport_authority`
   - `slice2::absent_receipt_status_tombstone_refuses_delayed_original_after_retry`
   - both recovery tests in step1
   - `slice2::active_claim_collection_reader_cannot_tombstone_held_owner_answer`
   - `slice2::lost_ack_reconciles_written_without_resend_or_rewriting_first_receipt`
   - `slice2::daemon_crash_after_claim_queries_tombstone_before_new_explicit_answer`
   - `slice2::written_typed_native_declines_preserve_once_only_no_workaround_ledger`
   Run each exact name via the same `--test pending_requests ... -- --exact --nocapture` command; bounded harness/worker waits and isolated fixture cleanup apply. Preserve initial RED and GREEN separately.
4. Storage controls: `cargo test -p overseerd --bin overseerd pending_requests::answers::recovery_tests:: -- --nocapture` must match exactly2 named tests: uncertain-page/reopen progression and superseded/terminal public-update refusal. These are direct storage eligibility tests, not native transport proof.
5. After core path passes and coordinator approves continuation, run existing authored actual turn/replacement/device/Needs/confirmation cases. List exact names with `--list` before any broad slice2 filter; report source/count and classify unreached assertions. Prioritize their already-frozen baselines rather than adding source-only cases. Simulated Voice requires its explicit isolated environment; do not run real audio/speech/provider tools.
6. Incumbent adapter/answer_waiting/gateway/protocol compatibility and eventual full/three-surface/browser gates require separately coordinated allocation. No arbitrary broad workspace/full/UI execution from this manifest.

## Bounds and cleanup

Use a parent subprocess deadline for each focused command (maximum5 minutes after build); inspect the active handle before polling. On timeout retain failure output, stop/join only this command's owned test fixture processes and verify no owned daemon/shim/workers survive. Never hide a timer/setup failure or release held claims merely to satisfy a progress assertion. Tests own kill9/restart data and proxy restore cleanup; independently inspect any failed unwind. Release the exclusive compiler slot immediately after the bounded batch.

## Acceptance limits

Check-only5b established syntax/type metadata, not executable behavior. Current recovery correction has only rustfmt parsing/diff-check evidence. Historical status queries cannot mint device pairing/scope, a current turn, fresh owner intent, grants, proposal confirmation or native execution. File approval grantRoot is not exact denied operation identity. Partial-write/dead-shim disposition, all receipt/rival/stale readback negatives, structured surface renderers and native/browser acceptance remain explicit required broader-plan gaps until their actual assertions run.
