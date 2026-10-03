# Core native answer qualification

Two coordinator-run phases establish eight scoped core cases; **there was no single all-eight GREEN run**. This editor only preserved existing output and authored evidence. No runtime was launched while publishing.

| Phase | Exact source | Result | Artifact provenance |
|---|---|---|---|
| Original batch 99144 | `9040eb52bc58baaf24685e1b08744e955c145982` | 7 passed, 1 fixture assertion failure | Package-only clean; standalone build 23.735s; integration/unit no-run 39.404s; all three executables fresh:false, source `.d` and SHA256 receipts retained |
| Corrected decline 53387 | `ca012224f58a2f22b66ff6e0931e6dbceeb2e705` | Exact decline test1/1 passed; both typed and legacy iterations completed | Changed integration test fresh:false in 1.093s; source hash and `.d` retained. Standalone was reused, not rebuilt: Cargo reports fresh:true, while its original fresh:false provenance belongs to9040. SHA256 `1c542a4dbc9adf765da1c540526bd2d70909a295f3bbff521b296e82c7b205f8` verified before/after by the coordinator |

Both runs used coordinator-allocated `/private/tmp/overseer-closeout-verify-target`, `nice -n 20`, one build worker and one test thread, separately from the full suite's UI-only phase. Exact inventories in each phase show one named test per command. Raw stdout/stderr, bounded-capture stage exits/timing, relevant original Cargo artifact lines, dependency files and hashes are attached. Full Cargo output and unrelated baseline process inventories are omitted; no raw output was fabricated.

## Qualified cases and scope

The original phase passed these seven exact cases:

- `slice2::native_token_tool_cannot_inherit_local_owner_transport_authority`: actual authenticated token tool entrance strips inherited LocalOwner despite forged argument fields. It calls the read tool `roster`; this is not blanket native protected-answer/proposal qualification.
- `slice2::absent_receipt_status_tombstone_refuses_delayed_original_after_retry`: a real delayed connection cannot write its tombstoned token after one actual retry write.
- `slice2::active_claim_collection_reader_cannot_tombstone_held_owner_answer`: collection reads preserve the live held claim; release writes one exact decline and denial row.
- `slice2::lost_ack_reconciles_written_without_resend_or_rewriting_first_receipt`: actual backend write with a dropped acknowledgement reconciles once without resend or mutation of the first result.
- `slice2::daemon_crash_after_claim_queries_tombstone_before_new_explicit_answer`: real daemon kill/restart reattaches its surviving shim, obtains a not-written tombstone, and waits for a fresh explicit answer.
- `pending_requests::answers::recovery_tests::unresolved_receipt_pages_advance_past_uncertain_rows_and_survive_reopen`: direct migrated storage pagination progresses past sixteen unresolved rows across reopen.
- `pending_requests::answers::recovery_tests::historical_attempt_cannot_settle_a_new_token_or_terminal_request`: direct eligibility checks refuse superseded tokens, changed revisions and terminal requests. The storage cases do not prove native transport.

Original case05 failed at the shared `reject` helper's zero-response assertion after the test intentionally wrote one valid frozen decline. The first typed write, one denied-operation row and cross-agent no-reroute refusal had passed; the compatibility iteration and remaining tail were not reached. This is a fixture mismatch, not a product authority RED. Its raw failure remains in the baseline directory and earlier [correction record](../core-fixture-correction/RESULTS.md).

At ca012, `slice2::written_typed_native_declines_preserve_once_only_no_workaround_ledger` passed the entire two-iteration loop: typed answer and legacy `run.permission` decline each write exactly one frozen response, preserve exactly one denied-operation row, refuse cross-agent rerouting, and reject a repeated answer as `already_answered` without another/different response or protected action. The corrected command took 4.598s including process startup; libtest reports 4.58s. Only that test changed; production, fixtures, shared negative helper and manifests were unchanged for this rerun.

## Cleanup and remaining qualification

Original raw cleanup reports ours 0/others 1. The coordinator identified unrelated transient Git PID 49854 and checked it had already disappeared (`ps` tool 4c7a0d exit 1, no output); it was never stopped. Corrected raw cleanup reports ours 0/others 0. These are phase-specific observations, not a claim about the owner's processes.

Earlier recovery and strict-family/seven-vector qualifications remain separately pinned to 79ca/e84; their evidence is not replaced. This eight-case core coverage does not close AC274. Captured Needs identity, same-generation rollover/completion, replacement generation, checked Overseer and simulated Voice confirmation, device scope/revocation/receipt cache/privacy, further answer families and incumbent adapter/answer/gateway/protocol/full/browser gates remain outside this batch until their actual assertions run. No installed-native, real-audio, provider or owner-profile qualification is claimed.
