# Gate S — the live probes (one tiny paid turn each)

Run on 2026-09-27 on the owner's Mac through the daemon itself (`overseerd serve` in an isolated
`OVERSEER_HOME`), with the owner's installed harnesses and their existing logins: Claude Code
2.1.246 with `haiku`, Codex with `gpt-5.6-luna` at low effort. Nothing of the owner's was
written; the probes' repository, worktrees and folders lived under `/private/tmp` and are gone.
The driver is `live-probes.py` (kept with the session's scratch); the raw results are in
`results.json` (paths and tokens redacted).

Paid turns: Claude Code `haiku` ×8 (a report, two guardrail turns, two redirect turns, Overseer's
own two turns, one watcher turn), Codex `gpt-5.6-luna` low ×6 (a report, two guardrail turns,
two redirect turns, one watched agent). One attempt each; none repeated.

| Verify clause | Probe | Seen |
| --- | --- | --- |
| AC-190: one tiny live report each from Claude Code and Codex | The agent, given the channel by the daemon (`--mcp-config` in its run folder on Claude Code; `-c mcp_servers.overseer.*` on Codex), is asked to call `report` | Both called `report` through the shim (`overseer_tool_call` on the run); the report is stored under the run's token with `doing: "live probe: idle"`; both replied `done` |
| AC-187: what each harness refuses; the label | A guardrail denying `src/` set after the first turn; the second turn asks for `src/probe.txt` with the file tool only | Claude Code: the label reads **enforced** (deny rules on its command line); the model replied `REFUSED` and no file was written. Codex: the label reads **watched** (no per-path switch exists); the model followed the words, replied `REFUSED`, and no file was written; a write would have been caught by the sweep (`guardrail_crossed`, AC-187's fixture test) |
| AC-188: one tiny live redirect each on Claude Code and Codex | The agent counts into `count.txt` one edit at a time; `run.redirect` after its first edit | Both: a snapshot was taken, the turn stopped (`delivery: "stopping, then the direction"`), the direction ran as the next turn (2 turns in all), and the model replied `redirected`. Claude Code had written `1` and was on `2`; Codex had written `1`. (The driver's own check expected the `From Overseer:` prefix, which an owner-sourced redirect does not carry; the redirect itself succeeded on both.) |
| AC-189: one tiny live check-in on Claude Code | Overseer's own run on `haiku`; check-ins every turn; a program agent that appends a line to `docs/keep.txt` | The check-in turn called `check_in` once for the agent: `done`, *Completed with 1 file modified (docs/keep.txt); status finished*, nothing left out; the conversation shows the turn's usage |
| AC-193: one tiny live watch, Claude Code watching a Codex agent | A Codex agent creates `hello.txt`; a watch with a `haiku` watcher, brief: only `hello.txt`, nothing else | One wake when the subject finished; the watcher read the subject's `changes` through its tools and filed **fine** |

What the probes did not cover: OpenCode (no live login was used; its read-only launch and its
tools were shown on the mock model in AC-180), a watch that checks on a live harness, and a
live report from OpenCode.
