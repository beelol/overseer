# Overseer audio tracks

The daemon bundles twelve original Reactor synth MP3 cues (34,224 bytes total). Every cue is shorter than 0.5 seconds. No game recordings, cloned speech, or model are bundled.

Audio Mode is off by default and needs one explicit enable. Its global track and enabled state persist in the daemon's local database. Only `agent_started`, `agent_complete`, and `agent_needs_attention` play automatically for top-level runs. Failed or disconnected roots use the attention cue because they cannot proceed. The other nine Reactor keys are previewable and silent by default. Child, Swarm, and detailed voice lines reuse broad keys; they do not create extra automatic sounds.

The selectable tracks are:

- **Reactor signals**: Play the bundled MP3 through macOS `afplay`. On first use, the selected asset is copied to a tiny owner-only local cache.
- **System voice**: Use macOS `say` on the device for “Agent started,” “Agent complete,” and “An agent needs your attention.” The user may choose an installed voice. No speech file or model is downloaded.
- **Private Commander**: Import a local folder containing the three core `transmission/commander.wav` clips. The daemon reads these in place with `afplay`; it never copies them into the repository, extension, cache, bucket, or network.

The Rust daemon classifies durable lifecycle events and owns the setting, playback queue, deduplication, and burst coalescing. VS Code only changes settings and requests previews. Closing VS Code or opening a second client does not create another player. The queue holds at most four routine cues and two urgent cues, one transient player process runs at a time, and attention deduplication history is bounded. Missing private files or playback errors are logged without interrupting agents. On other platforms the selected macOS audio track reports unavailable.

`manifest.json` lists the Reactor keys, meanings, provenance, and durations. The private voice-to-synth mapping remains in the separate Machines voice lab.
