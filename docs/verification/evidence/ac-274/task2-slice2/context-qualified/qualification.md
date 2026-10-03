# Native pending context qualification

Coordinator batch 71308 passed **15/15 exact headless integration cases** at `d7b6630132cdc8f074df0f6f7f2101b774c0444e`. Each case had an exact one-test inventory and a separate bounded command; all raw stdout/stderr and receipts are preserved in `qualified-d7b663/`. These are synthetic native fixture transport/daemon controls, not installed providers, real audio, browser or production-owner qualification. Publishing this evidence ran no tests or daemons and changed no implementation.

## Source and reused artifact proof

The d7b663 change from tested ca012 is evidence-only. The coordinator reused and hash-verified the same-clone daemon and test executables; **no new build is claimed**:

- Standalone daemon SHA256 `1c542a4dbc9adf765da1c540526bd2d70909a295f3bbff521b296e82c7b205f8`, originally fresh-built at 9040.
- Pending-requests executable SHA256 `83d6790072f2a3c12af0696da36aef08bd6b72ccddd8e34aaa7fc1be6cf6099c`, last freshly compiled at ca012.
- Test source SHA256 `85ab032b9fa43f5e6442f5d7c332b1e7f41c2192373b4f182caca459101a038e`; both source `.d` files are attached. Earlier actual fresh:false artifact records remain in [core qualification](../core-qualified/qualification.md). Receipt fresh:false fields describe that original build, not a new d7b663 compilation.

Execution used the coordinator-allocated separate verify target and one worker/test thread, with nice -n 20 commands and no UI/provider turns. Exact per-command exit, elapsed time, signal/timeout/output-cap/reap status and executable/source hashes are preserved. Baseline process-ID inventories are deliberately omitted from public evidence.

## Exact coverage

- `slice2::native_resolution_after_claim_before_send_prevents_reply`
- `slice2::captured_needs_yes_does_not_approve_replacement_native_request`
- `slice2::captured_needs_handle_does_not_propose_a_new_native_request`
- `slice2::needs_handle_reports_stale_preparation_after_initial_capture_recheck`
- `slice2::typed_bool_bridge_requires_frozen_revision_and_keeps_exact_native_decline`
- `slice2::checked_overseer_confirmation_freezes_daemon_revision_and_exact_native_item`
- `slice2::native_turn_rollover_same_generation_retires_held_answer_and_keeps_current_control`
- `slice2::native_turn_completion_retires_held_answer_before_process_settlement`
- `slice2::replacement_generation_after_claim_before_send_rejects_old_attempt`
- `slice2::permission_profile_widening_and_deny_removal_cannot_claim`
- `slice2::structured_questions_and_forms_refuse_legacy_bool`
- `slice2::native_resolution_before_owner_answer_emits_no_response`
- `slice2::old_request_key_cannot_send_to_replacement_generation`
- `slice2::native_child_typed_answer_uses_parent_pipe_and_exact_native_id`
- `slice2::exact_receipt_token_writes_once_and_refuses_changed_digest_or_generation`

The actual assertions cover resolution both before answer and after a held claim; captured Needs yes/Handle request identity plus the second stale-preparation boundary; the revision-frozen legacy bool bridge and refusal for structured questions/forms; checked Overseer proposal confirmation; actual same-generation turn rollover/completion and replacement-generation retirement; permission-profile widening/deny-removal refusal; old selectors versus replacement generation; child request routing through its parent pipe with the exact native ID; and once-only private receipt tokens versus changed digest/generation. Healthy replacement/current controls are retained where authored. This batch does not qualify every arbitrary race or authority surface by test name alone.

## Preserved setup attempt and cleanup

Before the qualified run, the restricted sandbox attempt failed daemon startup in its first three cases at `daemon/tests/common/mod.rs:116`; their authority assertions were not reached. The coordinator interrupted the fourth case and reported wrapper 99751 exit 130. Only the available original streams/inventories/receipt are attached in `sandbox-setup-interrupted/`; no missing fourth-case or wrapper output was fabricated. The sandbox also printed `nice: setpriority: Operation not permitted`, process enumeration errors and failed `pkill`; its reported empty cleanup alone is not successful process-inspection proof. These are setup/allocation failures, never product REDs.

The qualified run's actual cleanup JSON is `{"ours":[],"others":[]}`, with receipt ours 0/others 0. Commands and owned children were reaped according to the bounded receipts. This observation is separate from the failed sandbox enumeration.

## Remaining scope

Prior recovery, strict-family/seven-vector and [core qualification](../core-qualified/qualification.md) remain separately pinned and retain their original failures. Simulated Voice checked readback, authenticated device scope/downgrade/revocation and private once-cache/history/result disclosure cases remain outside this batch. Incumbent adapter/answer_waiting/gateway/protocol compatibility, installed-native and structured/rendered controls, shutdown/full/browser gates and the broader AC274 Verify clause still require their own actual evidence. No AC closure, ready-to-merge or full-suite claim is made.
