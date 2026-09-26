# Harness compatibility (macOS, this build)

What Overseer supports per harness, account path, version and capability — and how each
claim was established. "Live" means a real harness process with a real account; "mock"
means the real harness process driven by Overseer's deterministic mock model provider;
"fixture" means a recorded or synthetic transcript replayed through the adapter. Unknown is
never shown as zero or as supported. The same capability strings are shown in the UI
(**Overseer: Show Harness Capabilities** and each run's *Capabilities* section).

Verified on macOS 26.6.2 (arm64), VS Code 1.139.0. Evidence index: [verification ledger](verification/README.md).

| Harness (version) | Account path | Launch / output | Follow-up / resume | Interrupt | Permission requests | File activity | Native children | Usage / quota | Evidence level |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Codex** 0.155.0-alpha.16.4 (ChatGPT.app bundle; stale PATH `codex` 0.1.x ignored) | ChatGPT login via `codex login` per profile (`CODEX_HOME`); existing `~/.codex` login usable as "codex (existing login)" | ✅ `exec --json` | ✅ `exec resume <thread>` | ✅ SIGINT | ⚠️ none in the exec transport (sandbox policy); use `codex-app` for approvals | ✅ `file_change` items | ✅ depth 1 observed live (`collab_tool_call spawn_agent/wait`); child output limited to final message; use `codex-app` for streamed child output and grandchildren | usage ✅ (`turn.completed`); quota unknown (error text only) | **Live** single account ([codex-live](verification/evidence/ui/codex-live/)). Two simultaneous accounts **not verified** (AC-12). |
| **Codex (app-server)** same binary, harness id `codex-app` | same ChatGPT login/profiles as Codex | ✅ JSON-RPC over stdio (`initialize`, `thread/start`, `turn/start`) | ✅ `thread/resume` + `turn/start` | ✅ `turn/interrupt`, then SIGINT | ✅ **live**: command approval requests shown in the run panel; Allow/Deny/Interrupt verified; never auto-approved; unsupported server requests refused | ✅ `fileChange` items | ✅ **live** child + grandchild with `-c agents.max_depth=2` (child-thread items attributed to the child) | usage ✅ (`thread/tokenUsage/updated`), rate-limit/credit state ✅ (`account/rateLimits/updated`) | **Live** approvals ([codex-approval-live](verification/evidence/ui/codex-approval-live/)); ≈17k tokens per tiny turn vs ≈100k with `exec`. |
| **Claude Code** 2.1.246 | claude.ai login via `claude auth login` per profile (`CLAUDE_CONFIG_DIR`) | ✅ `-p` stream-json | ✅ `--resume` | ✅ control_request interrupt → SIGINT | ✅ **live**: `--permission-prompt-tool stdio`, Allow/Deny in the run panel | ✅ Write/Edit inputs | ✅ **live** child + grandchild (Agent ids, `parent_tool_use_id`, `system/task_*`); background subagents keep the session open | usage ✅ (`result`); quota via error classes | **Live** ([claude-live](verification/evidence/ui/claude-live/)), Max plan, haiku. |
| **OpenCode** 1.15.13 | provider login via `opencode auth login` per profile (`XDG_DATA_HOME`) — **not tested**; verified with a local mock provider configured in the profile | ✅ `run --format json` | ✅ `--session <id>` | ✅ SIGINT | unknown (`run` auto-rejects asks unless configured) | ✅ edit/write tool parts | ✅ children and grandchildren (task tool `metadata.sessionId` + session store `parent_id`), **live** with a local model | usage ✅ (step_finish tokens); quota unknown | **Mock** model through the real OpenCode runtime ([main](verification/evidence/ui/main/), protocol tests), plus local Ollama models: `qwen3-coder:30b` completed a write; `qwen2.5-coder:14b` emitted its tool call as text ([log](verification/evidence/ac-14/opencode-ollama.log)). No account authentication claimed. |
| **Generic executable** | none | ✅ argv (no shell), cwd = workspace, sanitized env | ✅ line to stdin | ✅ SIGINT | unknown | unknown — review refreshes from the filesystem; Follow shows a limitation note | unknown (shown as "Native children: unknown") | unknown | Protocol tests (AC-15). |
| **Gemini CLI** | Google sign-in (doc-only) | ❌ no adapter | — | — | — | — | subagents cannot nest (doc-only) | — | Not installed; documentation survey only (AC-01). Use the generic harness meanwhile. |
| **Devin** | `devin auth login` is Enterprise-only (doc-only) | ❌ skipped | — | — | — | — | — | — | Skipped per owner decision: no account-login path without API keys / personal access tokens (AC-01, AC-17). |

## Everyday parity (AC-60)

What people do in Claude Code or Codex directly, from Overseer's chat. "Live" means checked with a
real harness on 2026-09-26 (Claude Code with the existing login and haiku; Codex gpt-5.6-luna on
ChatGPT A); see [AC-60](verification/AC-60.md).

| Capability | Claude Code | Codex (exec) | Codex (app-server) | OpenCode | Generic |
| --- | --- | --- | --- | --- | --- |
| Model per turn | ✅ `--model` (live) | ✅ `-m` (live) | ✅ thread model | ✅ `-m` | — |
| Reasoning effort per turn | ✅ `--effort` low…max (live) | ✅ `-c model_reasoning_effort` minimal…xhigh (live) | unsupported (shown so) | unsupported (shown so) | — |
| Permission mode per turn | ✅ `--permission-mode` Ask first / Accept edits / Plan only / Auto (live) | ✅ sandbox Read only / Can edit (`-s`, or `sandbox_mode` on resume) (live) | approval policy at start | unsupported (shown so) | — |
| Attach or paste images | ✅ image blocks in the stream-json message | ✅ `-i <file>` | unsupported (shown so) | unsupported (shown so) | — |
| @-mention worktree files | ✅ named in the message; the agent reads them (live) | ✅ same (live) | ✅ same | ✅ same | ✅ same |
| Steer: queue a message | ✅ sent when the turn ends (Overseer) | ✅ | ✅ | ✅ | ✅ |
| Steer: stop and send (⌥Enter) | ✅ control_request interrupt, then resume (live) | ✅ SIGINT, then `exec resume` (live) | ✅ `turn/interrupt` | ✅ SIGINT, then `--session` | ✅ SIGINT |
| Continue after VS Code or the daemon restarts | ✅ `--resume <session>` (live) | ✅ `exec resume <thread>` (live) | ✅ `thread/resume` | ✅ `--session` | — (new process) |

Unsupported options are refused by the daemon with the reason, and the composer hides them for
that harness. Images are stored in the run's folder (mode 0600), at most 4 per message, 5 MB each.

## Environment and account rules enforced by the daemon

- Harness processes get an allow-listed environment (HOME, USER, PATH, locale, TMPDIR, XDG
  dirs). `*_API_KEY`, `ANTHROPIC_*`, `OPENAI_*`, `CLAUDE_CODE_OAUTH_TOKEN` and `*_ACCESS_TOKEN`
  are never forwarded and a profile cannot set them (`daemon/src/adapters.rs`, tests
  `base_env_drops_keys`, `forbidden_keys`).
- Profile status reports API-key logins as *not signed in* for Overseer's purposes.
- Overseer never logs out the existing (system) login; isolated profiles live under
  `~/Library/Application Support/Overseer/profiles/<id>` (0700).
- Observed on this machine: the shared `~/.codex/auth.json` (also used by the ChatGPT
  desktop app) switched between two different ChatGPT accounts during the session. Use
  isolated profiles when the account must not change underneath a run.

## Platform

Designed for macOS and Linux (platform paths in `daemon/src/paths.rs`, peer credentials
via `getpeereid`/`SO_PEERCRED` in `daemon/src/shim.rs`); only macOS is verified. Linux is
AC-41 and unverified.
