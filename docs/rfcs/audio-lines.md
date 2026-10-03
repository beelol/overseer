# Audio Mode: twelve lines and one pack format

Status: owner-directed standard, 2026-10-03. The vocabulary is approved; the complete classifier,
pack loader and source switch are required work, not implemented or verified by this document.
AC-275–AC-288 in [the main RFC](../overseer-rfc.md) track that work.

## Vocabulary and meaning

These are the only twelve Audio Mode notification keys and exact English lines. This scope is
Audio Mode notifications; conversational Voice Mode answers retain their own speech behavior.
The daemon classifies semantic state, then the selected pack resolves the key to a file. A pack
cannot add notification types or change their meaning. Legacy cue names are migration aliases,
not additional exposed or automatically played notifications. In particular, accepting or denying
a permission does not play `agent_unblocked` or `agent_stopped`: it is an action the owner just took.
This supersedes only the old Audio Mode permission-answer cue clauses in AC-171/172 and the
Voice Mode RFC. Their trusted confirmation, toast/Cancel, settle window, exactly-once delivery
and audio-arbiter obligations remain unchanged; conversational read-back still works. The AC-165
Heard signal remains dedicated, non-spoken Voice Mode feedback with its existing latency bound;
it must not expose or play an extra Audio Mode pack key such as `agent_queued`. Preserve its
visual acknowledgement, arbitration and no-self-capture tests.

| Key | Exact line | Meaning |
| --- | --- | --- |
| `agent_started` | Agent started. | A top-level agent actually begins its first work turn. |
| `agent_complete` | Agent complete. | A top-level logical task reaches successful completion with no continuing goal or pending validation. |
| `agent_permission_required` | Agent needs permission. | A live owner-actionable permission decision remains after permitted automatic handling. |
| `agent_reply_required` | Agent needs a reply. | A live agent question or required information needs an owner answer. |
| `agent_sign_in_required` | Agent needs you to sign in. | Authentication is required and no permitted signed-in route can continue. |
| `agent_cannot_continue` | Agent cannot continue. | Work is blocked with an actionable owner decision after available recovery is exhausted, without a more specific permission, reply or sign-in need. |
| `agent_failed` | Agent failed. | The logical task ends unsuccessfully after recovery is exhausted. |
| `agent_stopped_unexpectedly` | Agent stopped unexpectedly. | An active top-level agent loses its execution unexpectedly and cannot reattach or recover. |
| `agents_need_attention` | Several agents need attention. | At least two distinct top-level agents acquire unresolved owner-actionable needs within the coalescing window. |
| `swarm_initiated` | Swarm initiated. | An accepted Swarm director is running and ready to dispatch its work. |
| `swarm_complete` | Swarm complete. | The whole Swarm objective has passed its required completion and verification conditions. |
| `swarm_needs_attention` | Swarm needs attention. | The whole Swarm is blocked on a real owner action after the director exhausts permitted recovery. |

## Trigger coverage and silence

Maintain a checked-in trigger inventory for all twelve keys. The [initial source audit](../audits/2026-10-03-audio-twelve-trigger-inventory.md) records current producers and concrete gaps. Each row names the real producer,
transport/harness, raw event or derived state, recovery state, logical task/need identity, expected
key or silence, and the integration test/evidence. Enumerate every supported producer path,
including manual starts, Auto, Swarm, native pending requests, legacy adapters and daemon recovery.
An unclassified or unqualified path is a visible gap; a generic status test cannot stand in for it.
The per-line ACs below are the minimum families, not permission to ignore another real producer.

Only top-level logical work announces. Child/worker progress stays silent. Swarm announcements
replace the corresponding director/worker announcements. Overseer's ordinary self-started turns,
check-ins and watchers stay silent; its actionable owner needs use the same semantic classifier.
Deduplicate by logical transition/need, not merely identical sound within a time interval.
Several distinct agents becoming needy within the existing 800 ms window use the plural line.
Within a Swarm, its aggregate replaces individual needs. Across distinct top-level agents/Swarms,
the plural notice represents the distinct unresolved top-level needs, with the UI identifying them.

Specific actionable permission/reply/sign-in needs take precedence over generic cannot-continue.
Terminal failure and unexpected loss are distinct terminal causes; neither emits a second generic
attention line for the same transition. Retry, reconnect, reroute, account fallback and repair stay
silent while they can resolve the issue. Re-evaluate a queued cue immediately before playback:
resolved, superseded or obsolete needs and previous-daemon events never play. A genuinely new need
may announce after the old one clears. Repeated polling/reconnect events are not new needs.

## Identical folder contract

Both sources use this layout:

```text
<pack>/
  audio-pack.json
  audio/
    agent_started.wav
    ... one mapping for each of the twelve keys
```

```json
{
  "schema": 1,
  "id": "example-pack",
  "label": "Example pack",
  "lines": {
    "agent_started": "audio/agent_started.wav",
    "agent_complete": "audio/agent_complete.wav",
    "agent_permission_required": "audio/agent_permission_required.wav",
    "agent_reply_required": "audio/agent_reply_required.wav",
    "agent_sign_in_required": "audio/agent_sign_in_required.wav",
    "agent_cannot_continue": "audio/agent_cannot_continue.wav",
    "agent_failed": "audio/agent_failed.wav",
    "agent_stopped_unexpectedly": "audio/agent_stopped_unexpectedly.wav",
    "agents_need_attention": "audio/agents_need_attention.wav",
    "swarm_initiated": "audio/swarm_initiated.wav",
    "swarm_complete": "audio/swarm_complete.wav",
    "swarm_needs_attention": "audio/swarm_needs_attention.wav"
  }
}
```

`schema`, `id`, `label` and `lines` form the shared contract already agreed in the audio chat.
`lines` must contain exactly the twelve canonical keys. Values are relative paths to supported
WAV or MP3 files under the selected pack. Additional descriptive metadata (for example hashes,
duration and provenance from the lab) may be retained but never executed or treated as a new cue.
All twelve mappings must validate before selection. Each spoken file must contain its canonical
line completely, once, without a clipped first/last word or an extra generated continuation.
No missing-key fallback and no mixing clips from two packs. Invalid selection leaves the previous
valid selection unchanged; a selected folder that later disappears becomes visibly unavailable.
Playback failure logs a bounded reason and never blocks an agent or silently switches source.

The built-in pack lives at `daemon/assets/audio/default/` with this same manifest and audio layout.
Packaging includes only distributable, non-private assets. A standalone daemon may embed this
folder at build time and materialize only its own bundled files into an owner-only versioned
internal folder when enabled/previewed. This is a packaging detail, not another pack format or UI
choice. Keep one manifest-driven loader/resolver; do not duplicate a hard-coded key/file table.
The legacy Reactor preview vocabulary, Commander-specific folder shape and System track picker
are replaced by this contract. Old recordings remain historical evidence; new built-in files
must satisfy the twelve-line content checks. No runtime model/download is needed for playback.

A user-selected folder is read in place. Store the selection/path and validation metadata only;
never copy, import, cache, bundle, upload or commit its audio. In particular, POD files under the
private voice lab remain outside all worktrees and releases. Verification uses synthetic packs
unless the owner explicitly runs a local audition. Do not put private file paths or metadata in
public reports; source availability can be reported without disclosing the path remotely.

Validation is bounded: manifest at most 64 KiB, exactly twelve mapped keys, each file at most
8 MiB and 15 seconds, and no eager loading of the whole pack. Refuse absolute paths, traversal,
unsafe symlinks/replacements, special files, malformed/unsupported audio and oversized inputs.
Validate the opened file used for playback so a path swap cannot escape the selected folder.
Missing/changed files are revalidated without replacing the selected source. Synthetic tests
cover these limits and changing/removing a folder after selection. Starts and completions (agent or Swarm) use the routine lane; the other eight keys use the
urgent lane, classified by semantic key rather than filename. Preserve the Voice Mode arbiter
while checking freshness again after any wait. One transient player at a
time and existing bounded urgent/routine queues remain; all players are reaped on disable/exit.

## Source control

Use plain labels **Built-in** and **From folder**. `From folder` opens the local folder picker;
cancelling does nothing. Show the selected pack label, availability and twelve-line preview list.
The Audio Mode on/off switch remains separate and off by default. Changing source does not enable
Audio Mode. Preview is an explicit action that works while off and does not change enablement.
Selection persists across daemon restart and is shared by local VS Code/TUI controls; only the
daemon plays. Remote surfaces may inspect source/availability and send already-authorized controls,
but cannot choose a Mac path, upload clips or make another client play.

Switch atomically between validated packs. An already playing clip may finish; queued clips resolve
against the newly selected pack and current semantic state, without mixing or duplicate players.
Disabling Audio Mode stops playback and discards pending cues. Legacy settings migrate explicitly
without turning audio on: old internal/System selections point to Built-in; old private paths stay
private and require a valid twelve-line manifest before availability. Never silently replace a
missing private pack with Built-in. The UI explains an unavailable migrated choice and offers the
same two source options, without exposing internal protocol or asset names.

## Evidence

Per-line criteria require actual daemon event-to-player captures with synthetic distinguishable
files, semantic/dedup/suppression receipts, and coverage of every inventoried trigger family.
Pack/source criteria additionally require packaged UI/TUI controls, restart/multi-client tests,
path and format negatives, shutdown cleanup and a guided local twelve-line audition. The audio
chat's approved private recordings establish the owner's selection, not application integration.
Historical AC-143–145 evidence proves the original implementation only. Their old three-track,
three-automatic-key vocabulary is superseded by these owner-directed criteria.
