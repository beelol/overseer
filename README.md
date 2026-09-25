# Overseer

A local agent orchestration daemon in Rust, with a VS Code UI for account-based agent
runs, recursive subagent visibility, and live editable worktree review built on Branch Diff.

**Status: specification drafted; implementation has not started. Verified acceptance
criteria: 0 / 40.** The first usable release targets macOS and Linux, including two
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
5. Verify packaged behavior on macOS/Linux and dogfood it (AC-36–40).

Live verification requires two distinct ChatGPT subscription accounts, supported account
access for the other initial harnesses, and both target platforms. Devin's account-only
integration remains a feasibility question. No product tests have run or passed yet.

Auto routing and a terminal UI are later milestones. The current task completed the
project specification; it did not start an overnight implementation goal.
