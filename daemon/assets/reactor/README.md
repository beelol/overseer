# Reactor cue pack

These twelve original synthesized MP3 cues ship inside `overseerd` (34,224 bytes total). They were created in the separate Machines voice lab with oscillators; no game recording, voice clone, or generated speech is included. Every cue is shorter than 0.5 seconds.

Audio Mode is off by default. One explicit enable persists in the daemon's local database. The daemon then plays only `agent_started`, `agent_complete`, and `agent_needs_attention` for top-level runs, including a failed or disconnected root that cannot proceed. It selects cues from its own durable event stream, so VS Code can be closed. Repeated attention states and dense batches are coalesced. The other nine keys are included for preview and for future stable lifecycle events; they do not add automatic notifications now.

The key is the reusable sound identity. Detailed voice lines, child-agent labels, Auto Mode choices, and Swarm roles should map to one of these broad keys rather than create new sounds for every phrase. `manifest.json` lists the meanings and durations. The private Commander voice recordings remain in the separate lab.

Playback uses the macOS system `afplay` executable when a cue is needed. A tiny owner-only copy is written to Overseer's local data directory on first playback. No network connection or resident audio model is required. A bounded four-cue queue and one transient player process keep memory usage low. Other platforms report playback unavailable.
