# Audio Mode available lines and pack dictionary

Status: owner-approved vocabulary recorded for later implementation (2026-10-03). This document
extends the [Audio Mode RFC](audio-mode.md); it does not make the additional lines play yet.

## Purpose

Audio Mode speaks only short states that remain useful while the owner is looking away. The daemon
classifies the event into a stable **line key**. The selected audio pack maps that key to its own
file. This keeps application behavior independent of filenames, voices and sound design.

The spoken line identifies the state. Overseer's Needs you view explains the cause and actions.
Automatic recovery, retry, rerouting and reconnection happen before an attention line plays.
Only top-level agents speak. Child and worker activity stays silent.

## Available lines

These keys and English strings are the canonical Audio Mode vocabulary. Keys are stable protocol
values. Copy changes require an RFC update; a pack cannot redefine what a key means.

| Line key | English line | Plays when | Default Reactor asset |
| --- | --- | --- | --- |
| `agent_started` | Agent started. | A top-level agent actually begins work. | `agent_started.mp3` |
| `agent_complete` | Agent complete. | A top-level agent finishes successfully. | `agent_complete.mp3` |
| `agent_permission_required` | Agent needs permission. | The agent is waiting for Allow or Deny. | `agent_needs_attention.mp3` |
| `agent_reply_required` | Agent needs a reply. | The agent asked a question or needs information from the owner. | `agent_needs_attention.mp3` |
| `agent_sign_in_required` | Agent needs you to sign in. | No signed-in route can continue after automatic fallback is exhausted. | `agent_needs_attention.mp3` |
| `agent_cannot_continue` | Agent cannot continue. | The agent remains blocked after its recovery options are exhausted. | `agent_needs_attention.mp3` |
| `agent_failed` | Agent failed. | The task ends unsuccessfully after automatic recovery is exhausted. | `agent_failed.mp3` |
| `agent_stopped_unexpectedly` | Agent stopped unexpectedly. | A running agent exits without completing and cannot reconnect. | `agent_stopped.mp3` |
| `agents_need_attention` | Several agents need attention. | Two or more unresolved agent needs arrive together. | `agent_needs_attention.mp3` |
| `swarm_initiated` | Swarm initiated. | The director starts and can dispatch agents. | `agent_started.mp3` |
| `swarm_complete` | Swarm complete. | The Swarm finishes its objective successfully. | `agent_complete.mp3` |
| `swarm_needs_attention` | Swarm needs attention. | The whole Swarm cannot continue after its director exhausts recovery. | `agent_needs_attention.mp3` |

`agent_started` is an optional launch acknowledgement. The other applicable line plays once per
state transition. Several simultaneous needs collapse into `agents_need_attention`; they do not
play one line per agent. A Swarm line replaces its constituent agent lines for the same transition.

## Pack dictionary

Every file-backed pack has an `audio-pack.json` manifest. The `lines` object is the dictionary the
player resolves. Values are pack-relative file paths; more than one key may point to the same file.

```json
{
  "schema": 1,
  "id": "example-pack",
  "label": "Example pack",
  "lines": {
    "agent_started": "agent-started.wav",
    "agent_complete": "agent-complete.wav",
    "agent_permission_required": "needs-attention.wav",
    "agent_reply_required": "needs-attention.wav"
  }
}
```

Resolution is deterministic:

1. The daemon emits one canonical line key from the table above.
2. A file-backed track looks up that exact key in its selected pack's `lines` dictionary.
3. If it is absent, the daemon uses the key's **Default Reactor asset** from the table. This lets a
   small signal pack cover all events without duplicating files.
4. The System track speaks the canonical English line instead of resolving a file.
5. An unknown key or unsafe path is refused and logged; it never delays or fails an agent.

Paths must be relative, remain inside the pack folder after canonicalization, and name a supported
audio file. Pack import validates every declared file before selection. Playback remains bounded by
Audio Mode's existing queues and single transient player.

## Existing tracks

- **Reactor:** its current twelve bundled assets remain unchanged. The mapping in the table reuses
  them for the larger vocabulary.
- **System:** speaks the canonical line using the selected installed macOS voice.
- **Private Commander:** keeps its current three-file layout until imported packs adopt
  `audio-pack.json`. Its legacy adapter maps the three broad keys exactly as it does today.
- **POD and future private packs:** may provide a distinct file for every line key. They remain
  outside the public repository unless their rights and distribution terms are explicitly settled.

## Deliberately silent

Progress, tool calls, file changes, check results during active work, queued or resumed states,
individual child or worker events, actions the owner just took, brief disconnects, and failures an
agent is still handling do not produce spoken lines.
