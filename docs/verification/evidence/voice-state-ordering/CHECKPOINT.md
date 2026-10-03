# Voice state ordering: authored baseline and timeout observation

Base: frozen `fe64322`, in an isolated `codex/voice-state-ordering` clone.
No production behavior is changed. These two host tests are authored and **UNRUN**:

- `late_starting_snapshot_cannot_replace_live_listening`: defer a real Voice host
  `voice.get` snapshot of Starting, deliver the live Listening event, then release
  that older snapshot. Assert host state, status text and webview snapshot remain
  Listening. This targets the unconditional assignment in `Voice.refresh`.
- `off_state_then_toggle_explicitly_reenables`: deliver Off with the crash reason,
  invoke the real toggle, and require exactly one `voice.set {enabled:true}` plus
  a Listening host and snapshot. This distinguishes the cache ordering witness
  from the reproducible packaged scenario's actual re-enable behavior.

The packaged scenario retains the original Listening/Thinking predicate and
30-second timeout. Before the four-crash sequence an observation-only webview
listener retains at most 32 state/snapshot/listener-ready metadata messages. On
the unchanged timeout, it records its own daemon's enabled/state and listener
running/pid/restarts/last_error (bounded to 300 characters), alongside the DOM
state and that bounded message history, then rethrows the original failure. It
does not record speech, requests, proposals or model content. The observer is
removed after the wait. This script can be run against the original VSIX: no new
extension bundle is necessary for the diagnostic.

Existing isolated evidence is not a host-race runtime proof. The retained owned
fixture database has an Off voice_settings event followed by an enabled change,
and final `voice.enabled=1`; the error screenshot shows Starting. There is no
post-enable runtime `voice.get` receipt establishing whether the listener or the
host was stuck. Therefore a second accidental disable does not explain that
witness, and the deferred snapshot race remains a separate code-derived
candidate until the baseline and packaged diagnostic are executed.

No tests, daemons, listener processes, UI, provider turns or compiler commands
were run during this authoring checkpoint. Only source reads and `git diff
--check` were performed. Parent owns the next execution allocation. No Audio
branch or frozen observed checkout was edited.
