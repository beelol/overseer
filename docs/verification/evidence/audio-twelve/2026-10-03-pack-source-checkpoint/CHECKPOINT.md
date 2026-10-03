# Pack/source daemon source checkpoint — not runtime qualification

Base: combined baseline `038c2c41a70a7daedb92c9873b4b1a22bfc9ad9e`, then canonical Line dependency `ed947e5d38564fb5b698350937222e68752d9a1a`, normally merged. No ledger or criterion status changes.

Actual baseline evidence is the coordinator's `/private/tmp/overseer-audio-baseline-038c2c41`, published as `f714fbf73657dc9a70e7607843a528e9ed851738`. Eight pack cases failed: inventory was null; six selection cases stopped at unknown `audio.source.set`; the legacy source was null. No compiler/setup/timeout failure. Later negative/preview assertions did not execute and are not individually RED-proven.

This checkpoint authors descriptor-rooted in-place manifest/media resolution, persisted validation metadata, revisioned source/toggle settings, legacy selection projection, a supervised AudioToolbox decoder and opened-descriptor player. `audio.get` performs bounded manifest reads and descriptor metadata checks; it does not repeatedly decode all twelve media files. The synthetic sink validates the actual opened media before writing `pack-id:key`.

Source-only checks: rustfmt parsed the new focused modules and fixtures; generated protocol freshness and git diff whitespace checks passed. No Cargo check/build/test, decoder worker, native player, daemon, UI or paid call ran for this checkpoint.

Four extra API boundary fixtures are authored and UNRUN: manifest/media/duration bounds; truncated WAV/nondecodable MP3; parent symlink/FIFO; strict source fields and toggle revisions. They need allocated actual baseline/control runs before any regression-fix claim. Deterministic parent/root replacement, exact-limit positives, MP3 success, worker timeout/memory/cleanup and native descriptor playback still need focused fixtures and runtime qualification.

Integration boundaries are explicit:

- Canonical queue/runtime dependency is not integrated here. Preview refuses with a truthful reason; it never acknowledges admission into the legacy three-cue queue. Existing semantic subscriber/Cue/play source is deliberately retained for the semantic agent's separate normal merge. This intermediate checkpoint is not a shippable complete Audio Mode implementation.
- Native descriptor playback qualification is false. Folder selection can be validated/stored, but normal native availability/enablement refuses until isolated afplay `/dev/fd/0` seek/playback proof exists. No pathname/copy fallback.
- Decoder deadlines (two seconds per file, twenty-five seconds per selection) and observed-RSS kill threshold (96 MiB) are provisional and unmeasured. An RSS watcher is not a hard instantaneous allocation cap. Codec acceptance/frame-count consistency and worker cleanup need actual synthetic WAV/MP3 evidence.
- Metadata identity detects changes; it is not immutable-byte proof against in-place modification. Changed selected files remain selected but unavailable and require reselection at this checkpoint. Automatic revalidation of changed/migrated folders remains pending.
- Built-in assets are unavailable pending the owner's content clarification and approved distributable twelve-line files. No private audio was read, copied, generated, cached or bundled.
- Local VS Code/TUI controls, gateway tests, cancellation/shutdown integration, rendered/terminal checks, guided audition and full combined suite remain pending.

## Source review follow-up (unrun)

Two concrete source ordering findings were identified in `9e3489a`: an older enable could apply its runtime flag after a newer disable, and a decoded old-pack descriptor could be admitted after a new source committed. Baseline observation/test checkpoint `3e409e0` adds bounded postcommit and afterdecode gates; the next test-only checkpoint adds cancellation and actual permission-resolution negatives. None has run: no RED/GREEN claim is made, and canonical queue integration is required for the afterdecode cases to reach their intended assertions.

The source correction uses one short admission/settings guard. Folder decoding/metadata inspection remain outside it; source commit and runtime callback are serialized after Store release; final player admission checks committed revision and semantic freshness under the same guard. A live cue may re-resolve once against the latest source after discarding its old FD outside the lock. Repeated churn logs an explicit skipped-cue error; cancellation/resolved need returns unplayed false. Source changes after actual admission may let the in-flight clip finish. The native player, decoder budgets/limits and runtime cleanup remain unqualified.

## Dedicated Heard source checkpoint (unrun)

The AC-165 nonspoken exception reuses the existing approved 2,684-byte cue (SHA-256 `37fe68af5b1f3782a285d90428d90345645c529fa8f2f9182cb64be2a945dd11`) outside Line/pack mappings. `player::heard_feedback` shares serial playback/cancellation and final enablement admission; the semantic worker owns the preceding Voice arbiter/epoch check. Its separate synthetic receipt is `feedback:heard` in `OVERSEER_TEST_VOICE_FEEDBACK_LOG`. Native playback uses only the existing distributable bytes on an unlinked owned descriptor and remains gated/unqualified. No private media, generated asset or thirteenth notification was added.

A pure asset-fidelity/no-extra-Line unit check is authored UNRUN. Actual Voice acknowledgement, latency/arbitration/no-self-capture, off/disable and combined-runtime tests are still required; logging alone is not audible qualification. The existing visual heard_signal and conversational acknowledgement caller remain the semantic agent's integration area.

A separate reviewed shutdown follow-up remains necessary: decoder children are not yet centrally registered, and the incumbent server exits after 100 ms. Do not count current cancellation as decoder/shutdown cleanup proof.
