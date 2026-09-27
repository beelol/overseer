# AC-180 — the spikes before Gate S is built

Run on 2026-09-27 on the owner's Mac (macOS 26.6.2 arm64, Git 2.54.0 Apple, Node 24.20.0) with
the daemon at the tested commit, an isolated `OVERSEER_HOME` holding 4, then 16, fixture agents
(the Claude fixture in `showcase` mode), and one tiny live turn per harness where a fixture could
not answer the question. Paid turns: Claude Code `haiku` ×2 (the first lost to a driver mistake),
Codex `gpt-5.6-luna` at low effort ×2 (the first refused by Codex's approval policy), OpenCode
with the mock model ×1. Transcripts are redacted (`<owner>`, `<scratch>`, `<token>`).

## 1. Tools from the daemon, without touching the user's configuration

The daemon gives a run tools through `overseerd mcp --socket <path>`, an MCP server over stdio
(newline-delimited JSON-RPC) that forwards every call to the daemon (`overseer.tools`,
`overseer.tool`) with the run's token from `OVERSEER_MCP_TOKEN`. The daemon decides who is
speaking from the token; the shim holds nothing of its own.

| Harness | How the run is given the server | Tool names as the model sees them | Result |
| --- | --- | --- | --- |
| Claude Code 2.1.246 | `--mcp-config <run's file> --strict-mcp-config` (the file lives in the run's folder; `--strict-mcp-config` ignores every user-configured server) | `mcp__overseer__roster`, `mcp__overseer__agent` | listed in `system/init` with `mcp_servers: [{overseer, connected}]`; `roster` called and answered ([transcript](claude-tools-readonly.jsonl)) |
| Codex 0.155.0-alpha.16.4 (`exec --json`) | `-c mcp_servers.overseer.command=… -c mcp_servers.overseer.args=[…] -c mcp_servers.overseer.env={OVERSEER_MCP_TOKEN=…}` plus `-c mcp_servers.overseer.tools.<tool>.approval_mode="approve"` per tool | `roster`, `agent` on server `overseer` | `mcp_tool_call` item completed with the roster; "There are 16 agents." ([transcript](codex-tools-readonly.jsonl)). Without `approval_mode` the call fails: *MCP tool call requires approval, but approval policy is never* ([first attempt](codex-attempt1-approval-never.jsonl)). The app-server transport has an `mcp_tool_call_approval` request instead, which the daemon can answer. |
| OpenCode 1.15.13 (`run --format json`) | `mcp.overseer` (`type: local`, `command`, `environment`) in the profile's own `opencode.json` under the profile's `XDG_CONFIG_HOME`, the file Continuity already writes for local accounts | `overseer_roster`, `overseer_agent` | `opencode mcp list` shows `overseer connected`; the mock model called `overseer_roster` and got the roster ([transcript](opencode-tools-mock.jsonl), [list](opencode-mcp-list.txt)) |

The user's own configuration after the spikes: `~/.claude/settings.json` unchanged since
2026-09-10; `~/.claude.json` has no `mcpServers` entry named overseer and no entry for the spike's
folder (its project entries are Claude Code's own bookkeeping of worktree folders);
`~/.codex/config.toml` has no `mcp_servers` entry (its `[projects.*]` entries are Codex's own
trust records); `~/.config/opencode/opencode.json` unchanged since 2026-05-07.

## 2. Keeping a run read-only

| Harness | How | Seen |
| --- | --- | --- |
| Claude Code | `--disallowedTools Bash,Edit,Write,MultiEdit,NotebookEdit,WebFetch,WebSearch,Agent,Task,TodoWrite,KillShell,BashOutput,ToolSearch,AskUserQuestion` and `--allowedTools` naming Overseer's tools; the run's folder is empty | `system/init` lists no Bash, Write or Edit; the model reported "there's no Bash/shell or Write tool available"; nothing written in the folder. In `plan` mode the model also tried `ExitPlanMode`, so Overseer's run uses the default mode with the tools removed instead. `can_use_tool` still arrives for an MCP tool even when `--allowedTools` names it: the daemon answers it (allow for Overseer's own tools, deny anything else), which is what the live run did. |
| Codex | `-s read-only`; the shell stays available inside the sandbox (Codex has no switch for it) | a write attempt was refused: *patch rejected: writing is blocked by read-only sandbox*; the model reported "Writing x.txt was not possible (read-only workspace)"; nothing written |
| OpenCode | `"tools": {"bash": false, "write": false, "edit": false, "patch": false, "multiedit": false, "task": false, "webfetch": false}` in the same `opencode.json` | the mock run offered only the MCP tool (the request's tool list carried `overseer_roster`); nothing written |

## 3. A message to a working agent, and whether it was picked up

No new probe: the existing evidence stands ([docs/compatibility.md](../../../compatibility.md),
AC-60). At the end of a turn: Claude `--resume <session>`, Codex `exec resume <thread>` or
`thread/resume` + `turn/start`, OpenCode `--session`. In the middle of one: Claude
`control_request interrupt` then resume, Codex SIGINT then resume, OpenCode SIGINT then
`--session`. What is new in Gate S is only where the queue lives (the daemon, AC-188).

Picked up: a tool call through the shim reaches the daemon with the run's token, so a *report*,
*ask* or *claim* from an agent is attributed by the daemon and never parsed from text. Every call
became an `overseer_tool_call` event on the calling run in all three spikes (and in the replay
test `ac180_mcp_shim_serves_overseers_tools_from_the_daemon`). For a harness whose run has no
tools, the card can only say *delivered*, then *answered* when the next turn ends.

## 4. What an Overseer turn and a check-in cost

| Measure | 4 agents | 16 agents |
| --- | --- | --- |
| Roster (one line per agent) | ~1.0 KB (~260 tokens) | 4.2 KB (~1,050 tokens) |
| Digests (fixture agents, no changes yet; bounded at 4 KiB each) | 1.2 KB (~310 tokens) | 5.0 KB (~1,250 tokens) |
| Live Claude Code haiku turn with the roster: `cache_creation 57,252 + cache_read 113,204 + input 6, output 477` over 3 model iterations, 10.1 s, $0.26 | measured | — |
| Live Codex luna turn with the roster: `input 68,852 (51,456 cached), output 117`, 11 s | — | measured |

What this says: the roster and the digests are small; what a turn costs is the harness's own
system prompt and tool catalogue (about 57k tokens per model iteration on Claude Code, 69k per
turn on Codex exec, most of it served from cache). A check-in reads one digest (≤ 4 KiB) and
answers in one iteration, so it costs about one harness baseline, not more. Two consequences for
the design: Overseer's session is kept warm and resumed (`--resume`, `exec resume`) so the
baseline is a cache read rather than a cache write; and the roster stays one line per agent.
The roster call itself took 1.3 s for 16 agents because `workspace_changes` ran `git status`
in every worktree: the digest of AC-183 keeps changed-file counts from events instead.

## 5. A trial merge in a 10,000-file repository

`git merge-tree --write-tree --name-only <a> <b>` between two agents' commits (30 files each,
two of them changed on the same lines, one on different lines): **15 ms**, exit 1, naming the two
conflicting files; the same with `--merge-base <base>`: 12 ms; a clean pair: 11 ms; `git
diff-tree --name-only -r base..a`: 12 ms. No worktree, index or branch was touched. Conflict
detection for 16 agents (120 pairs, only changed pairs recomputed) fits well inside AC-192's
10 s.

## Decisions written into the RFC

See [Spike results](../../../rfcs/orchestrator.md#spike-results-ac-180): the shim and tokens as
built; per-harness delivery and read-only settings; the daemon answers Claude's `can_use_tool` for
Overseer's own tools; Codex agents on the exec transport get `approval_mode="approve"` for
Overseer's tools; Overseer's session is kept warm; digests keep counts from events.
