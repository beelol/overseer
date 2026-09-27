# AC-200 — security review of Gate S (Overseer itself)

Reviewed on 2026-09-27 by the agent that built the gate (pull request #14), against the code on
its branch. A second pair of eyes is the owner's call; every finding below has its resolution in
the code and a test that shows it.

## What the design relies on

| Rule | Where the daemon enforces it | Shown by |
| --- | --- | --- |
| Who is speaking comes from the token, never from the text | `overseer_tool` resolves the run and role from the token's hash (`overseer_tokens`); a call names no sender; a report with an `agent` argument is stored under the token's run | `ac190_briefing_and_channel` (a token from one run cannot report as another), `ac200` (a forged token is refused) |
| The model never touches an agent, a worktree or a shell | Overseer's run is launched read-only (Claude: `--disallowedTools` for every shell, file, web and delegation tool; Codex: `-s read-only`; OpenCode: `tools` off); its only tool with an effect is `propose`, which the daemon checks | `ac184` |
| Every action has a class and the level decides | `ACTION_CLASSES`, `METHOD_CLASSES`, `overseer_propose`: a Confirm action needs the owner's own turn and a yes at every level; at Ask first nothing happens before a yes; at Steer only quiet actions go by themselves | `ac185`, `ac186`, `ac200` (a Confirm action from a finding's turn is refused at every level) |
| A watcher only reads its subject | `SUBJECT_READS` held to the watch's subject; the watcher's tools have no `propose` | `ac193` |
| Reads stay inside a worktree | `inside_worktree`: relative paths only, no `..`, symlinks resolved and checked | `ac184` |
| No credential enters a digest, a report, a share, a finding or the conversation | `redact` at the door of the channel (report, ask, answer), on every tool answer, on shares, on session messages and digests | `ac200`, `ac191` |
| Caps hold whatever the words | `overseer.cap` on self-started turns; twelve wakes an hour per watch; a check-in queue of twenty per turn | `ac198`, `ac193`, `ac200` |
| A denial is never worked around | `denied_permissions` and `denied_match` in `overseer_propose` | `ac196` |
| Shares go only where the owner allows | `share_denials`; across repositories a Confirm action | `ac191` |

## Findings and their resolutions

1. **Channel messages were stored as sent, so a credential in a report reached the digest's JSON
   (the text form was redacted, the JSON was not).** Resolved: reports, questions and answers are
   redacted before they are stored; the test `ac200` reads the digest as JSON.
2. **Overseer's own run was given a briefing and a started card like an agent, because its role
   was recorded after its first event.** Resolved: `task.create` takes the role with the request,
   so a run of the daemon's own is never treated as an agent, not even for one event.
3. **Briefings changed every agent's task once two worked in a repository, whether or not the
   owner had ever spoken to Overseer** (Continuity's fixtures broke). Resolved: the default gives
   briefings and the channel only once Overseer's run exists; the owner can still turn them on or
   off per agent or for all.
4. **A watcher's read of another agent was possible with a watcher token.** Resolved: the daemon
   holds every read of a watcher to its subject and refuses the rest as a tool error.
5. **A stop finding with hold on stop did nothing when the subject was idle** (the hold only
   stopped a running turn). Resolved: the hold is recorded either way, so the next turn waits.

## What is not covered here

- The live probes (one tiny turn each) of what each harness refuses when launched read-only are
  recorded in AC-180's evidence; they were not repeated for this review.
- The phone's class table (`protocol/protocol.json`) is on pull request #10's branch; the daemon's
  own table is the source the phone will read.
- The review is the builder's own. The owner may ask for another.
