# AC274 Task1 — unconnected native request codecs

Source commit: `94348ec8bc1d70bfb34bf8250a0cc009d890f950`. Prior codec/counter-guard checkpoint: `6e8e09a50755acb0a39062b130144910b454fb4e`. Base `44398746e113cb822bfef5dd376021b6f47117b3`; frozen vector/baseline authoring commit `fa3c6078316ce6b0220e383b2716b9a48dd2957c`. This is codec/fixture qualification only. No newly decoded family is connected to Norm, shim replies, the attention slot or runtime dispatch. These tests do not establish that the current native approval path is fixed or that AC274 is verified.

The private codec selects Codex0.158.0 or Claude2.1.288 explicitly, preserves native string/int64 identity, method/context/envelope, and separates owner, machine, resolved and unsupported families. Native ID/context/envelope Debug output is redacted and payload structs are not publicly serializable. Tagged answers validate exact offered decisions, whole native grant descriptors, permission subsets with denies retained, question identities and bounded form fields before constructing method-specific responses. Native persistent veto and default_to_no metadata remain intact. Unknown dialogs/modes cannot fabricate cancellation; machine callbacks cannot acquire owner permission. Accepted URL/openAI form/verification results remain unqualified.

## Actual baseline and correction failures

`zero-match-setup.log` exited0 but matched zero tests: the shared retired target reused a prior clone's unit artifact. It is invalid RED/green evidence. Only overseerd package artifacts in that exclusive target were invalidated, retaining dependencies and leaving the active full-suite integration target untouched.

`baseline-adapter-red.log` then compiled the authoring source and ran exactly five named tests against the old adapter boundary: four intended assertion failures (legacy accept vs approved, decision vs granted profile/scope, fractional native ID becoming a permission, decline-only request receiving Always), plus the signed/string native-ID control passed. This is the actual initial RED. The same frozen inputs/expected outputs moved to the separate typed codec; production legacy functions were not changed.

`first-codec-attempt.log` records6/7: the complete vector test rejected a valid Codex form because the bounded validator did not yet recognize installed double/uint32 schema annotations. The exact formats were implemented, including the uint32 bound. `codec-checkpoint-green.log` then records9/9.

Self-review subsequently found malformed Claude dynamic-form counter limits could be ignored. `form-limits-red.log` reproduces the exact native accept for minLength=-1 where Unqualified was required. The minimal guard requires the same unsigned-integer representation used to enforce minLength/maxLength/minItems/maxItems. The final test checks24 malformed cases (negative, fractional, uint64 overflow, string, null and bool across four keywords) plus8 valid constraints, including zero and finite uint64 ceilings. This counter-guard checkpoint preceded the numeric precision correction below.

## Successful checks

All Rust commands used `CARGO_TARGET_DIR=/private/tmp/overseer-closeout-verify-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 nice -n 20`, one builder in assigned slots. Exact source paths in compile output and exact test names/counts were inspected.

| Command suffix after cargo test -p overseerd | Result | Exact log |
|---|---|---|
| `--bin overseerd ac274_native_vectors:: -- --nocapture` (final precision source) | 12/12;102 frozen cases:59 exact native responses,40 refusals,3 resolved; distinct family/machine/unsupported, context/privacy/veto, malformed limits and exact numeric precision controls | numeric-precision-vectors-green.log |
| `--bin overseerd ac274_native_vectors:: -- --nocapture` (prior counter-guard checkpoint) | 10/10 | codec-final-green.log |
| `--bin overseerd adapters:: -- --nocapture` (final precision source) | 34/34:12 codec,7 adapter,7 native grant,8 turn-option/Mods/protected-role controls | numeric-precision-adapters-green.log |
| `--bin overseerd adapters:: -- --nocapture` (prior counter-guard checkpoint) | 32/32 | all-adapters-final-green.log |
| `--test answer_waiting -- --nocapture` (prior codec checkpoint before the private form guard) | 18/18; existing permission arbitration/session grants/veto/legacy controls; guard is unconnected and does not alter this runtime path | answer-waiting-green.log |
| `--bin overseerd adapters::tests -- --nocapture` (prior checkpoint) | 7/7 | legacy-adapter-green.log |
| `--bin overseerd adapters::always_allow_tests -- --nocapture` (prior checkpoint) | 7/7 | grant-controls-green.log |

`python3 fixtures/transcripts/ac274/check-vectors.py` passed:102 unique sanitized vectors,54 request objects and49 response objects conform to their frozen Codex schemas;21 verbatim schema hashes unchanged. The remaining10 positive responses use sanitized Claude installed-source-derived expectations, not a complete formal Claude schema. `rustfmt --edition 2021 --check daemon/src/adapters/native_requests.rs` and `git diff --check` passed. No focused Cargo/test/daemon/shim process remained after compiler release.

## Remaining gates

Task2 must connect the codec to daemon-owned generation/session identity, concurrent pending collections and atomic claims. Task3 must provide public redaction, secret-field/outbox rules, correct shared surface controls and default_to_no behavior; stored native context alone does not authorize a caller. Current capability claims for older versions do not qualify these installed codecs. Richer form schemas are refused for acceptance while known decline/cancel remains available; accepted external proof/browser routes, native cache matching, actual installed transport/provider/browser execution, live permission revocation and full integration remain unqualified. No new capability is claimed usable through runtime until those paths are connected and tested.

No UI, paid turns, credentials/profile access or production daemon work occurred. Root owns full-suite scheduling and criteria/ledger main edits. Independent source review accepted the bounded Task1 slice, including final precision source94348ec. This branch remains draft pending later lifecycle/runtime and integration gates.

Coordinator review found `as_f64` could round distinct integer minimum/maximum operands above2^53 into acceptance. `numeric-precision-red.log` ran exactly the two new named tests against unchanged source6e8 and failed both: minimum9007199254740993 accepted9007199254740992, and an unsafe float bound was silently qualified. Source94348ec compares signed/unsigned integral operands exactly through i128; finite mixed-float operands inside absolute2^53 compare exactly or by range separation. Unsafe large float bounds are explicitly unqualified, and unsafe float values in numeric bound checks are refused. This is bounded codec support, not complete JSON Schema numeric support. The tests cover eight signed/unsigned misses with equal-bound controls, four unsafe float bounds, four unsafe mixed values, four valid small/range-separated mixed cases and two fractional-bound misses. Final source compiled from this clone and passed12 codec/34 adapter tests; earlier logs remain evidence only for their recorded checkpoints. Coordinator independent final source review accepted precision source94348ec and evidence checkpointdd0464aa. This acceptance covers only the disconnected Task1 codec; AC274 and broader runtime/full gates remain partial.
