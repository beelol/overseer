# Overseer

A local agent orchestration daemon in Rust, with a VS Code UI for account-based agent
runs, recursive subagent visibility, and live editable worktree review built on Branch Diff.

**Status: specification drafted; implementation has not started. Verified acceptance
criteria: 0 / 41.** Design targets macOS and Linux; verification currently targets macOS only, including two
simultaneous ChatGPT subscription accounts. There is no runnable Overseer build yet.

## Project plan

- [RFC and authoritative acceptance checklist](docs/overseer-rfc.md)
- [Verification ledger and evidence requirements](docs/verification/README.md)
- [Inspected sources and reuse assessment](docs/source-assessment.md)

The RFC contains confirmed user decisions, explicitly proposed defaults, exact Follow /
live Review / base-comparison semantics, feasibility gates, and a draft implementation goal.
Only verified behavior gets checked off; implemented-but-untested and blocked work stays open.
Update this summary and the ledger together when criteria are verified.

## Delivery order

1. Validate account isolation, native child visibility and reuse choices (AC-01–03).
2. Build the durable daemon and VS Code controls (AC-04–10).
3. Integrate accounts/harnesses and recursive visibility (AC-11–20).
4. Implement safe workspaces and live editable review (AC-21–35).
5. Verify packaged behavior on macOS and dogfood it (AC-36–40); leave Linux AC-41 unchecked.

Codex and Claude Code must use account login. OpenCode may initially be verified with
mock responses or a very small local Qwen Coder through Ollama. Paid verification prompts
must be tiny and use minimal tokens. Two OpenAI accounts exist, but agent login access is
unproven. Skip Devin if account login is unavailable. Missing native child telemetry stays
visible and unchecked without making an otherwise usable harness inaccessible.

New agent runs default to changes since that run started; task-start, original fork and
other-branch comparisons remain selectable. Follow pauses on navigation until resumed.

**Planning only: explicit confirmation is required before implementation or harness tests.**
The proposed eight-hour future implementation session is separate from runs inside Overseer;
confirm that interpretation at start. No goal or overnight run is active. Auto routing and a
terminal UI remain later milestones. No product tests have run or passed yet.
