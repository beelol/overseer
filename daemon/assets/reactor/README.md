# Overseer audio tracks

The daemon bundles twelve original Reactor synth cues as MP3: 31,488 bytes together, each
shorter than 0.5 seconds. They are the owner's selection from the Machines voice lab, bundled
byte for byte and not regenerated here. No game recording, cloned speech, WAV file or model is
bundled.

<!-- pack:start -->
| Key | Meaning | Plays by itself | Seconds | Bytes | SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `agent_queued` | Waiting to begin. | no | 0.350 | 2,684 | `37fe68af5b1f3782a285d90428d90345645c529fa8f2f9182cb64be2a945dd11` |
| `agent_started` | A top-level agent begins. | yes | 0.365 | 2,828 | `7f8416a67056f790241bb7296ec8d35d47196c08d6f900aac16cfbf9bdcdf64c` |
| `agent_resumed` | An interrupted session continues. | no | 0.365 | 2,828 | `d76daca3fa30650fca55505102871be2a5119c54a30e9ff2191658286321fe5a` |
| `agent_progress` | Routine work advances. | no | 0.365 | 2,828 | `246dfd07208475625c7f71c41f86be4938f11d3bd213c72f9eee3f9e117ea829` |
| `agent_complete` | A top-level turn finishes. | yes | 0.365 | 2,828 | `167d213ae6b976d9f4bb8ed233352c1bc5cc0eb090c9e6c37ae3808efd0eaa7d` |
| `agent_stopped` | A run pauses or ends without completion. | no | 0.333 | 2,540 | `f3fabcc836a4b470b15e96e0dade012e62bd71d0fe00696dd6635d2bfd561a7d` |
| `agent_needs_attention` | A permission, login, or blocker requires you. | yes | 0.333 | 2,540 | `d35944ae8f64d8fb0476551f9a6ba948f9ef18b0e821b119b3e2cd4ae5aebda6` |
| `agent_failed` | An operation fails. | no | 0.333 | 2,540 | `b3df4298da0afed2bf3ee10fa27b7387db823d8ad62c6be9c8693b55d709bdfd` |
| `agent_unblocked` | Permission, account, or route becomes available. | no | 0.276 | 2,252 | `7b75354a71059ff7f83eb9765686359c67f4a571924c18f392c4900e0aa1b5cd` |
| `review_ready` | Changes or evidence are ready to inspect. | no | 0.333 | 2,540 | `f5e5036c4fdcde894c81a913514c29ae06c908d5a88eb04c3e15274681134cee` |
| `delivery_ready` | A PR or merge result is ready. | no | 0.333 | 2,540 | `75a6e03d29ccf3888b7ff2b317c460b519805950d2a848169ea0653bf21ac9cb` |
| `verification_passed` | Checks or acceptance evidence pass. | no | 0.333 | 2,540 | `648b499f4fd4072345153853c75c6299521ef095968bd75176929fe5bb3145a4` |
| 12 cues | | 3 | | 31,488 | |
<!-- pack:end -->

`manifest.json` holds the same facts and is what the daemon reports through `audio.get`. A
unit test compares every entry's size and SHA-256 with the bytes built into the daemon.
`python3 docs/verification/evidence/ac-138/check-pack.py` decodes every file, measures its
length and checks the manifest, this table and the repository; with `--approved <folder>` it
also compares each file with the owner-approved folder.

Audio Mode is off by default and needs one explicit enable. Its global track and enabled state persist in the daemon's local database. Only `agent_started`, `agent_complete`, and `agent_needs_attention` play automatically for top-level runs. Failed or disconnected roots use the attention cue because they cannot proceed. The other nine Reactor keys are previewable and silent by default. Child, Swarm, and detailed voice lines reuse broad keys; they do not create extra automatic sounds.

The selectable tracks are:

- **Reactor signals**: Play the bundled MP3 through macOS `afplay`. On first use, the selected asset is copied to a tiny owner-only local cache; a cached file whose content differs from the bundled cue is replaced.
- **System voice**: Use macOS `say` on the device for “Agent started,” “Agent complete,” and “An agent needs your attention.” The user may choose an installed voice. No speech file or model is downloaded.
- **Private Commander**: Import a local folder containing the three core `transmission/commander.wav` clips. The daemon reads these in place with `afplay`; it never copies them into the repository, extension, cache, bucket, or network.

The Rust daemon classifies durable lifecycle events and owns the setting, playback queue, deduplication, and burst coalescing. VS Code only changes settings and requests previews. Closing VS Code or opening a second client does not create another player. The queue holds at most four routine cues and two urgent cues, one transient player process runs at a time, and attention deduplication history is bounded. Missing private files or playback errors are logged without interrupting agents. On other platforms the selected macOS audio track reports unavailable.

The design and its bounds are in the [side RFC](../../../docs/rfcs/audio-mode.md); the acceptance criterion is AC-138.
