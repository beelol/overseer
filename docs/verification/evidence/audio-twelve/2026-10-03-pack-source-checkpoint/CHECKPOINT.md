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
