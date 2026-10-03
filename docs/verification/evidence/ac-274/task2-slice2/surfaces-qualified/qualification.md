# Reference-phone qualification and preserved Voice setup failure

Coordinator batch60353 selected six exact integration cases at `be0807d8e3eff1340c47540ccdf8f5b8318201a3`: **five loopback reference-phone cases passed; simulated Voice failed during setup**. Every exact inventory, original stdout/stderr, bounded-capture receipt, `.d` and artifact hashes are attached. This publication ran no test/build/daemon and changes no implementation.

## Five qualified reference-phone cases

- `slice2::full_device_downgrade_before_claim_cannot_send`
- `slice2::revoked_full_device_before_claim_cannot_send`
- `slice2::device_revocation_after_claim_before_send_prevents_held_reply`
- `slice2::authenticated_full_watch_and_revoked_devices_do_not_borrow_actor_text`
- `slice2::secret_typed_answer_is_private_in_results_history_and_device_once_cache`

These use authenticated reference-phone protocol clients over owned loopback gateway fixtures, not a rendered or installed phone. They qualify actual full→watch downgrade, revocation before/after durable claim, authenticated full/watch/revoked identity despite forged display actor fields, and isolation of typed secret answer data from public result/history/device once-cache. Only the reached assertions are claimed; broader UI/rendering/native-provider capabilities remain pending.

## Voice setup failure and correction plan

`slice2::voice_checked_readback_keeps_typed_revision_and_exact_native_decline` failed at `voice.set` with “the listener (overseer-listener) is not installed next to the daemon”. No readback revision, confirmed_voice provenance or exact native-response assertion ran; this is not a product authority RED.

The case sets only `OVERSEER_VOICE_SIMULATE=1`. Production `voice::set` still requires a listener, and simulated input does not itself replace output synthesis. Existing `daemon/tests/voice.rs` provisions a listener and sets an explicit path, synthetic voice and synthetic cue log. The approved test-only correction will make this pending case self-contained with a strict successful same-source listener build into the allocated target, explicit `OVERSEER_LISTENER`, `OVERSEER_LISTENER_TEST_VOICE=1`, and a disposable `OVERSEER_TEST_AUDIO_LOG`. No failed-build/stale-binary fallback, real microphone, native synthesis or private clips. Existing permission/readback/revision/provenance/native-response assertions stay unchanged. That correction is not part of this evidence checkpoint and remains UNRUN.

## Provenance and cleanup

This evidence-only source reused the coordinator's exact ca012 test artifact and unchanged9040 daemon: executable hashes and source `.d` are preserved, and no fresh build atbe0807 is claimed. The original fresh:false records remain in [core qualification](../core-qualified/qualification.md). Each command matched one test. Actual cleanup JSON is `{"ours":[],"others":[]}` (ours0/others0). Baseline process-ID inventories are omitted. Prior core/context phases retain their original setup and helper failures separately. No AC274 closure, full-suite, real-native, real Voice audio or packaged surface qualification is claimed.

Subsequent separate [Voice qualification](../voice-qualified/qualification.md): corrected test-only source3fe passed the exact case with fresh listener/test proof and unchanged assertions. The original setup failure above remains unchanged historical evidence.
