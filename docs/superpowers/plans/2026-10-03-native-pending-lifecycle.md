# Native Pending Lifecycle Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans and test-driven-development in the coordinator-assigned slot. This refines Task2 of the approved harness-pending plan; it does not replace Tasks3/4. Review this concrete design before runtime wiring. All new names below are proposed interfaces, not existing APIs.

**Goal:** Keep every delegated native request individually pending and send at most one schema-correct answer to its actual process generation, without restoring or retrying ambiguous delivery.

**Architecture:** Add a private durable request collection beside runs; project only redacted typed fields into shared state/events. The daemon validates a stored request before its atomic claim, and the shim records generation-bound delivery receipts for a new reply operation. The legacy permission method is an adapter for compatible bool permissions, never structured/external input.

**Tech Stack:** Existing Rust/SQLite/stdio shim and protocol JSON/TypeScript generator; no new provider/browser dependencies.

**Spec:** docs/overseer-rfc.md AC274; docs/superpowers/plans/2026-10-03-harness-pending-requests.md Tasks2–4; Task1 draft PR73 source94348ec, frozen head382b4f4. Branch codex/native-pending-lifecycle in /private/tmp/overseer-native-pending-20261003 starts from382b4f4; Task1 stays untouched. This plan is source-only and its tests are unexecuted.

## Global constraints

- Native owner/provider/OS/browser authority stands. No caller-supplied native IDs, session/generation, native descriptor, raw response, actor or capability qualification confers authority.
- Preserve finished Queue/Mods/native session grants, protected worker/tool/permission behavior, all other Gate S method classifications and voice confirmation provenance. Queue pause never resumes from a native answer.
- No paid/provider/browser/profile/production access or UI during full-suite lock. Future focused commands use nice20, jobs2/threads2, assigned exclusive absolute target. Compiler currently belongs to Mods TUI.
- Task2 alone does not qualify structured renderers, native browser availability, native revocation or every installed route. AC274 remains partial; no ready/merge request until coordinated full gates.
- Existing explicit later AC263 comparison labels stay unchanged.

## Current source boundaries

| Existing path | Concrete behavior requiring change |
|---|---|
| daemon/src/adapters.rs parse_codex_app836, parse_claude949 | Native owner frames become one generic Permission; unsupported Codex calls get generic error and Claude cancellation/dialogs fall through. Task1 codec is deliberately disconnected. |
| daemon/src/daemon.rs apply_lines2763/apply_norm2826, Permission3021 | Output cursor and events commit together, but a Permission overwrites runs.attention. Deferred state.sends runs after the store lock and ignores stdin failure. |
| daemon/src/store.rs claim_run_attention1714/unclaim1726 | One string request_id and one slot; error restoration can overwrite a newer request. permission_answers key omits generation. |
| daemon/src/daemon.rs answer_permission_with2555/send_stdin2399/control_socket2393 | Claims then looks up the current run's process, not the stored request's owning generation; any error is treated as proof nothing arrived. |
| daemon/src/shim.rs handle_control368, stdin433, control_with_timeout479 | write_all may write partially; ack can be lost after successful write. Existing helper cannot distinguish these from definitely unsent. Reply bytes are also recorded into raw segments. |
| daemon/src/daemon.rs state_for3799/raw_output3907, apply_norm2831 | Run serde exposes attention; normalized events use decoded redaction, but schema-declared secret fields and arbitrary native envelopes require an allowlisted projection rather than regex-only redaction. |
| daemon/src/gateway/remote.rs handle134/class check151/dispatch worker176 | Authenticated device identity/scope read before blocking worker; typed claim must recheck revocation/scope at its mutation boundary. Actor text is display, not authorization identity. |
| daemon/src/overseer/control.rs METHOD_CLASSES81; voice/request.rs readback1293/answer1399 | run.permission remains Confirm. Existing voice readback understands only bool permission attention; questions/forms must not be routed through yes/no accidentally. |

## Review focus

1. Two requests on one transport, including equal item with different approval IDs or integer7 versus string7, cannot overwrite or answer each other.
2. A held response races process restart/native resolution/device revocation: the operation's stored generation and authority must still match before the claim/send; no newer process receives stale bytes.
3. Timeout/partial write/daemon crash after claim cannot trigger a duplicate write, false resolution or automatic replay.
4. Native secret questions/form content, token refresh and URL credential queries cannot enter public state/events/errors/raw-output RPC or device dedup history.
5. Unsupported dialogs/machine requests/external authorization remain observable and correctly classified; opening a URL or receiving a write ack never proves the protected operation completed.

## Private identity and storage

Create daemon/src/pending_requests.rs, registered from daemon/src/main.rs. Keep SQL migration/API plumbing in store.rs; keep codec answer validation in adapters/native_requests.rs.

Proposed private `PendingRecord` fields:

- `key`: daemon-generated opaque UUID prefixed req-, primary key; this is the public selector.
- `process_run_id`, `display_run_id`, `process_generation`: read from the actual tail/launch and current Store, never model input. A native child may be displayed separately but replies always use its parent's actual pipe.
- `protocol`: frozen qualified codec selection from launch-resolved harness/version/transport. Existing Run.harness_version is recorded at creation and is insufficient if the executable changes before launch: capture the exact selected launch program's bounded version probe in launch metadata, and select only explicitly qualified versions. Failed/unknown version gets visible unsupported status, not a guessed decoder. Test fakes expose explicit --version without any provider turn.
- `native_id_kind`, `native_id_value`: private tagged int64/string; `method`, full `context` and native `envelope`; no generic Serialize or payload Debug.
- `native_identity_digest`, `offer_digest`: canonical private descriptor/context hashes for duplicate/conflict checks; public opaque key is independent. No digest supplied by a caller is authority.
- `revision`, `arrival_seq`, `created_ms`, `lifecycle`, `reason_code`; `claim_actor`, authenticated claim provenance, `claim_ms`, `answer_digest`, `delivery_token`, `delivery_state`, settled source seq/time. Full answers need not be stored durably: persist a digest plus private request/receipt metadata; public history contains no answer content. Never rebuild/resend an answer after a crash from its digest.

Migration uses CREATE TABLE IF NOT EXISTS plus indexes under existing Store migration conventions. Unique outstanding native identity is scoped to owning process/generation and tagged raw ID; it is not itemId or display label. Same ID + same envelope replay deduplicates, preserving key/order/revision. Same live ID + changed context/offer is a protocol conflict: preserve the original and expose a separate sanitized unsupported diagnostic, never overwrite authority. Same raw ID reused in a new process generation gets a fresh key; old key cannot claim. Same-generation ID reuse after settlement must be explicitly distinguished by a qualified native lifecycle, otherwise remains fail-closed as ambiguous (record this support limit).

`store.insert_pending(...)` and source cursor/event insertion occur in the same existing apply_lines transaction; replay after a crash does not duplicate pending items. Public order follows arrival_seq. Request revision increases only for lifecycle/projection changes, never changes an existing native offer into new authority. Terminal rows retain enough identity/delivery history for replay arbitration, while private payload cleanup follows run retention with no live request pruned. Empty/terminal rows do not count as pending.

## Public typed projection and methods

Proposed `PendingRequest` tagged union exposes opaque key/revision, display agent, process owner association, family/capability, redacted target/scope, native-offered choices, default_to_no/suppress_always, lifecycle and actionable reason. No raw native ID, envelope, tool input, auth refresh payload or native response is serialized. Native question identities map to daemon-issued field keys; translate back using the stored private mapping. Secret fields expose a secret flag, never saved answer/default values; unsafe external URLs expose safe origin/title and a blocked/open-step reason, not credential-bearing query/userinfo.

Minimal protocol in Task2 (Task3 later consumes it):

- `run.requests` read method, optional run_id, returns ordered public requests and cursor; include hidden shared Overseer only through its authorized session/read projection, consistent with current hidden-role behavior.
- `run.request.answer` control method `{run_id,request_key,revision,answer:RequestAnswer}`, typed answer discriminator and typed disposition `{request_key,revision,lifecycle,delivery}`. No arbitrary native message channel. Bounded lengths/content, deny unknown fields; encoder must succeed against the private stored request before claim.
- `pending_request`/`pending_request_changed` events carry only public union/lifecycle; answered summary contains actor and disposition, not secret answer content. No whole envelope in error.data.
- Add optional `Run.pending_requests` plus pending-item count; legacy attention projects the oldest compatible bool permission and its opaque key. Other typed families expose a safe pending descriptor/actionable unsupported-renderer reason, never `kind:permission` with bool buttons.
- `run.permission` maps a typed opaque key only to compatible command/file/Claude tool allow/deny choices actually offered by that request; Always must map exact native offered descriptor and veto. Reject permission-profile/questions/form/external/unsupported/machine families. For legacy unqualified transports retain existing bool flow without importing legacy label-only grants into new authority. Multiple typed items cannot be selected by ambiguous original raw IDs.

Register the new answer method as Gateway Control and Gate S Confirm (same owner permission boundary as run.permission); reads as Read/Look. Keep it unavailable to model-autonomous tool invocation absent the existing owner-confirmed path. Do not make arbitrary structured yes/no decisions available to voice. Adapt voice/Overseer bool compatibility to request key+revision from a readback bound to the same item; Task3 owns richer readback/renderers and default_to_no keystroke behavior.

## Atomic claims and send boundary

Proposed `claim_answer` validates fresh stored row, expected revision, generation/session/child transport identity, lifecycle, codec/offer, authenticated device scope and actor provenance under the existing store mutex/transaction. Encoder validation is pure; parse/validate outside the lock if necessary, then compare the exact record/offer digest/revision again before claim. Persist claim+answer digest+delivery token atomically. First accepted claim wins; losers see sanitized immutable first disposition. A claimed request remains countable as awaiting delivery/outcome rather than disappearing.

Use a dedicated per-owning-process serialization gate for answer and lifecycle launch/end transitions (key process run, not display child). Acquire gate before store; never hold store while shim I/O. apply_lines takes the same process gate before its existing store transaction when it can insert/resolve native requests; it never acquires the gate while holding store. Answer releases store for I/O and rechecks resolution/generation under the gate before send. A resolution already committed by the tail prevents the reply; a frame not yet applied cannot be claimed as an observed cancellation. A native cancellation concurrently arriving after bytes are sent is represented as such; no impossible guarantee of undoing native receipt.

Freeze generation-specific run dir/socket from the claimed record. Do not call today's control_socket(run), which reloads current process. Shim validates expected generation from its launch file; a stale lookup cannot hit a replacement process. Generation-changing launch/restore/end paths settle old records before publication, using the same ordering gate without reentrant task/queue locks. Existing stable task gate for Queue/Continuity remains separate: outer task gate → owning process gate → store; tails/answers do not take a task gate. Private non-reentrant launch/interrupt helpers must be used when the outer caller already holds a gate. No path may acquire task/process gates after store. Review actual complete call graph before code, including park/handoff callbacks after apply_lines releases the process gate.

## Honest delivery: new typed shim operation, legacy stdin unchanged

Add proposed `request_reply {generation,delivery_token,answer_digest,data}` and `request_reply_status` only on the private shim socket. Extend LaunchFile with process_generation (serde default for older shims) and use the exact owning process. Old shims lacking this typed boundary cannot claim guaranteed typed delivery; report unavailable until an isolated new generation is launched. No automatic restart of owner processes.

The shim serializes one token with its stdin mutex. Private receipt storage records `prepared` durably before writing, then `written` after full write/flush, without storing raw response in public segments. A repeated token+same digest returns the existing receipt and never writes twice; changed digest is refused. A crash/partial write/flush failure after prepared remains `uncertain`, never eligible for blind replay. Receipt/query errors are static, no native data in messages. Bound retained receipt count/bytes by process lifecycle; do not prune tokens still associated with open/uncertain records.

Daemon classifications:

- socket connect failed before any control bytes, or authoritative shim `not_written` before touching its pipe: restore only this still-live row to pending, preserving other items and requiring a new explicit owner answer; no automatic send.
- full pipe write/flush acknowledged: `answered_awaiting_native_outcome`, not operation success. Other unanswered items keep run waiting.
- any partial control write, timeout, EOF/malformed ack after write begins, prepared-only receipt or failed flush: `delivery_uncertain`; keep first claim immutable and refuse repeat answer. Restart queries receipt only, never auto-resends.
- native resolved/cancelled or authoritative matching turn/process end: settle individually with observed evidence. Explicit process loss without confirmed exit leaves unknown, not confirmed cancelled/completed.

These states replace the existing claim→any send error→unclaim assumption for typed requests. Existing generic stdin/metadata behavior need not change. Delivery receipt/dedup proves transport write count, not harness consumption or protected operation completion; those require native/mock capture.

## Lifecycle and privacy integration

Decode recognized control frames before legacy parse only for qualified launch protocol; each frame is consumed once. Preserve unrelated Session/Usage/Quota/child/Mods/Queue norms. Codex serverRequest/resolved settles by tagged request ID+owning generation+matching thread. Claude control_cancel_request settles exact stored request; unsupported request_user_dialog does not receive fabricated cancelled. Completion settles only requests belonging to that observed native turn/child; interim Claude background completion cannot clear a different live child's item. Confirmed process end settles all records for that generation; disconnected/lost process marks unknown and refuses further replies until actual same shim identity is re-established. No invented wall-clock expiry from deprecated autoResolutionMs; expire only with authoritative native deadline/cancel/outcome evidence.

Machine callbacks never become owner cards: configured host service routing stays separate; unavailable Codex machine calls with valid raw IDs get exact native unsupported errors and a redacted capability diagnostic. Unknown Claude callbacks/dialogs preserve observable unavailable status without fabricated success/cancel. Do not persist auth-refresh token response or native credentials into public store/events.

Use explicit projection allowlists first, then existing daemon::redact_value as a second pass. Keep raw native frames private. Raw-output RPC must suppress native control request/reply/auth frames, including unknown dialog/control envelopes, instead of regex-redacting their arbitrary contents. Shim typed reply logging records safe token/state metadata only; no answer content in s:i raw segments. Existing native request output needed for tail stays in private 0600 process storage, never returned through public raw-output. Field mappings/defaults/content get schema-declared secret suppression; device once-cache receives sanitized disposition only. URLs require userinfo/query handling from Task3 safe-opening policy; no automatic opening in Task2.

For phone mutation, carry a trusted device ID/scope in server request context rather than trusting `phone:<name>` actor strings or caller surface. Recheck current full scope and revocation under the same store lock as the claim. A request queued before revocation cannot send later. Read/watch projection stays sanitized; disconnected/reconnected dedup has no duplicate native response. Existing remote mutation once-cache cannot substitute for native claim identity. Secret answer content is never included in remote_command event or cached result.

## Bounded TDD slices (no tests run for this plan)

### Slice1: Collection, identity, projection and compatibility

Files: new daemon/src/pending_requests.rs; store.rs migration/APIs; main.rs module; adapters.rs normalization; daemon.rs apply_lines/state; protocol/protocol.json plus existing gen-ts.mjs output; new daemon/tests/pending_requests.rs; frozen fixture emit modes in existing fixtures/fake-harness/{codex-app-fixture.js,claude-fixture.js}.

- [ ] Author exact daemon tests `two_native_requests_preserve_order_and_tagged_ids`, `same_item_distinct_approval_requests_do_not_overwrite`, `replayed_frame_preserves_key_and_cursor`, `live_duplicate_id_changed_offer_conflicts_without_authority`, `same_raw_id_after_generation_restart_refuses_old_key` and `native_child_reply_uses_owning_parent_transport`.
- [ ] Author privacy tests `secret_questions_and_unknown_control_frames_never_enter_public_state_events_raw_output`, `machine_callbacks_remain_unavailable_without_owner_card`, `unknown_dialog_does_not_fabricate_cancel`.
- [ ] Actual RED first in assigned Cargo slot; exact count/source names. Missing proposed methods count as deliberate baseline failure, not completed implementation.
- [ ] Add private collection/projection/codec normalization and only required protocol/read paths; preserve legacy attention compatibility. Generator/checker green and protocol_shapes union negative assertions.
- [ ] Run new subset plus existing adapters/answer_waiting/protocol before source checkpoint review.

### Slice2: Atomic answers, provenance and delivery ambiguity

Files: pending_requests.rs; daemon.rs/server.rs; shim.rs; gateway/remote.rs; overseer/control.rs; voice/request.rs compatibility; pending_requests.rs/gateway.rs/answer_waiting.rs tests.

- [ ] Author `competing_answer_claims_emit_one_exact_native_response`, `wrong_family_scope_revision_or_offer_emits_no_response`, `changed_command_host_scope_session_and_generation_never_reuse_display_label`, `persistent_veto_refuses_stale_always`, `question_and_form_cannot_be_answered_by_legacy_bool_or_voice_yes`.
- [ ] Gate tests `resolved_before_send_prevents_stale_reply`, `restart_before_send_does_not_write_to_successor`, `revoked_or_watch_device_before_claim_cannot_send`; authenticated full device versus local competing claim asserts exactly one correctly typed native response and no protected deny action.
- [ ] Shim tests `connect_failure_is_definitely_unsent`, `partial_write_and_lost_ack_stay_uncertain`, `same_delivery_token_writes_once_and_changed_digest_refuses`, `crash_after_claim_queries_receipt_without_resending`. Gates only explicit synthetic fixture env, bounded waits, no store lock held.
- [ ] Actual RED, minimal send/receipt/provenance implementation, same GREEN; include original gateway arbitration, permission_card_answers, answer_waiting, Gate S and voice provenance controls.
- [ ] Native unresolved after ack remains awaiting; matching authoritative resolution settles. No model/provider activity required for these fixtures.

### Slice3: Cleanup/restart qualification and published checkpoint

- [ ] `cancel_one_keeps_other_pending`, `matching_turn_end_preserves_other_child_turn`, `process_loss_is_unknown_not_success`, `daemon_restart_replays_pending_but_never_answer_bytes`, `first_answer_result_is_immutable_across_reconnect`, `unsupported_external_accept_stays_unqualified_without_fake_completion`.
- [ ] Once scheduled: focused pending_requests full file, gateway affected controls/full file if changed, original answer_waiting/protocol_shapes and private shim units; generators/source checks. Exact chosen commands/counts before Cargo; no full/UI until coordinator grants.
- [ ] Review stable source/lock order/private projections and durable RED/green evidence. Publish draft separate from frozen PR73. Parent owns main criteria/ledger and eventual full integration.

## Remaining gaps after Task2

Task3 shared structured renderers/Needs identity navigation/default_to_no controls/secret outbox, Task4 installed browser/extension/provider handshake and authoritative external completion, rich unsupported forms/dialogs, native cache/revocation and full installed transport qualification remain explicit. Version probes/schema fixtures establish protocol selection only; they do not prove availability or native operation behavior. No universal no-repeat or browser usability claim follows from a shim receipt.
