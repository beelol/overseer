# Voice reply capture ordering — 2026-10-03

Verifier: Codex. Synthetic harness and simulated voice only; no microphone, paid turn, production daemon, UI, or full-suite run for this correction.

Source baseline: `780c5b9dceec8f6a1609e77e70926d7bbdc05e60`, the normal merge of current main into PR #52. The source and these logs are committed together. This is focused regression evidence, not complete AC-229/230 or full-suite verification.

## Reproduction

A fixture-only barrier in the ordered session consumer pauses assistant-output capture, while the harness emits its normal result and the raw run becomes completed. With the old voice lifecycle, both one-input question and aside cases became answered before their reply was captured ([capture-red.log](capture-red.log)).

The separate cancelled-predecessor case initially reproduced a second concrete error: withdrawal for V-0001 attached its proposal to the still-open V-0002 and cancelled it ([withdrawal-red.log](withdrawal-red.log), successor request event). Carrying the predecessor's immutable context through withdrawal closes this path too.

The barrier requires both `OVERSEER_TEST_NET=1` and `OVERSEER_VOICE_SIMULATE=1`; `OVERSEER_TEST_SESSION_CAPTURE_GATE` points to an existing temporary directory. The consumer writes `reached`, then waits for `release` for at most 30 seconds. It holds no store or voice lock. Production has no delay.

## Results

- [voice-green.log](voice-green.log): 14/14 targeted voice tests passed in 45.03 seconds. Six new capture tests cover delayed question/aside, empty success and failed result (one terminal event each), completed successor reply ownership, a predecessor's check-in Auto-start authority versus a later owner turn, and cancelled predecessor versus an open successor. Existing AC-166, AC-228, AC-229, AC-230 and AC-248 controls also passed in this run.
- [permission-green.log](permission-green.log): 11/11 passed, comprising seven permission lifecycle scenarios and four shared fixture helper tests. This retains ordinary decline, native denial, stale/mismatched card, timeout and multi-surface queue behavior.
- [protocol-green.log](protocol-green.log): one actual-event test passed. It checks emitted reply/completion against their strict protocol descriptions, identical immutable turn metadata, the expected request and owner cause, and reply sequence before completion sequence.
- `node protocol/gen-ts.mjs --check` and `git diff --check` passed.

Commands use `CARGO_TARGET_DIR=/private/tmp/overseer-voice-capture-target CARGO_BUILD_JOBS=2 nice -n 20 cargo test -p overseerd --jobs 2`, with `--test voice -- ac_capture ac166_the_right_agents ac228_ ac229_ ac230_ ac248_ --test-threads=2 --nocapture`, then the `permission_card_answers` test file, then `protocol_shapes captured_overseer_reply_and_completion -- --test-threads=2`. Test daemons are isolated and torn down by their fixtures.

Independent source review checked the locked settlement/event batches, durable preceding `turn_started` identity, output replay deduplication, and immutable request/cause propagation through replies, proposals and withdrawal. No remaining evidence-backed blocker was found. The existing voice subscriber's lag behavior and restart recovery are unchanged and outside this patch's proof. The coordinator owns final combined-head full verification; this patch does not claim it.

## Separate bounded CI qualification

One disposable diagnostic at baseline `715ee85b`, instrumentation commit `19513223d2188d35f2ecef5817aca72596fa1011`, ran in [Actions run 37106716008](https://github.com/beelol/overseer/actions/runs/37106716008). It kept the native and original TUI timing assertions and made exactly one synthesis attempt through each API. It used the same image version as the failed CI run, `macos-26-arm64 20260907.0351.1`, macOS 26.6.2 / build 25G83, on a different physical runner.

Native synthesis and independent `say` both produced 29,019 mono 16 kHz samples, peak 0.727752685546875, for the fixed phrase “On it. Telling Phone to wait.” Native busy reached 1 then 0; 349 write callbacks delivered 58,106 bytes. Numeric native voice identity was available, but legacy description returned -50, so its display name was unresolved. This probe does not explain the prior one-sample/85-sample failures or prove every runner is qualified.

The isolated original Follow assertion still failed at 268.120375 ms against 250 ms (first file 227.643209 ms). Disk observation to file-activity was about 170 ms; hunk RPC about 39 ms. This cannot be dismissed solely as full-suite concurrency. No native/TUI product fix, assertion relaxation, timeout increase or retry is included here. Detailed logs remain in the run artifact and `/private/tmp/overseer-ci-native-probe-37106716008`.
