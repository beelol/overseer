# Source assessment

Inspected 2026-09-24. Source reading establishes integration candidates, not that Overseer
already implements or passes any acceptance criterion.

## Branch Diff: reuse first

Repository: [beelol/branch-diff](https://github.com/beelol/branch-diff).
Inspected commit: `fbc6eb807fd41d8fd1a004977e1aa637a4f7c900`.

At that revision:

- `LICENSE` is MIT with Artem Kotov and Bilal Itani notices. Preserve these when reusing code.
- `package.json` describes a JavaScript VS Code extension and requires VS Code `^1.136.0`.
  Check installed editor compatibility before adopting it; do not silently lower this minimum.
- `extension.js` resolves merge-base comparisons and collects untracked changes.
- `review/comparison.js` watches repository/document changes and maintains versioned comparisons.
- `review/panel.js` reconciles visible reviews periodically (2.5 seconds) and manages sessions.
- `review/editing.js` uses VS Code edits, disk/document checks and draft recovery to avoid
  silently losing conflicting edits. The README documents editable stacked Monaco diffs.

Consequences: reuse the comparison/editor UI and its protections before rebuilding it.
Add an explicit selected-worktree integration, account/run context and Follow controls.
Verify the staged/unstaged cancellation case and external-worktree discovery independently.
The existing 2.5-second reconciliation interval is relevant to the RFC's proposed 5-second
missed-watcher bound. No Branch Diff UI or test suite was run during this documentation pass.

## Harness coverage and account constraints

[OpenCode provider documentation](https://opencode.ai/docs/providers/#openai) documents
ChatGPT Plus/Pro account authentication. This makes it a candidate for the accounts-only
scope; it does not prove concurrent profile isolation or native child telemetry.

[Devin API overview](https://docs.devin.ai/api-reference/overview) describes a remote REST
integration with service-user credentials and personal access tokens. It does not establish
a local CLI/account-login adapter. The current no-API-keys constraint therefore leaves the
integration unresolved; do not equate a subscription with a usable account-only control API.
Owner decision in the planning revision: skip Devin unless account login is available;
do not request API keys or personal access tokens for this release. OpenCode initial
verification may use mock responses or a very small Qwen Coder through Ollama, with coverage
labeled accordingly; this does not establish OpenCode subscription-login isolation.

Codex, Claude Code, OpenCode and Gemini CLI still need version-pinned live capability probes.
“100% feedback” cannot be verified by looking at a terminal screenshot or fabricated tree.

## Other reuse candidates

These are discovery links from the earlier conversation, not approved dependencies or
verified license assertions. Their code/licenses must be inspected at a pinned revision
before adoption under AC-03:

- [Agetor](https://github.com/alamops/agetor): evaluate account/session/worktree design if it reduces work.
- [Parallel Code](https://github.com/johannesjo/parallel-code): evaluate worktree/review integration.
- [Pane](https://github.com/greenfield-inc/Pane): inspect license obligations before copying any code.
- [XCB](https://github.com/hraness/xcb): its current README describes a metaharness/account-custody
  project with native XCB in development. Evaluate later routing/account reuse only if its
  tested interfaces match Overseer's interactive-session requirements. It is not the selected foundation.

A Rust daemon with a small adapter boundary is the proposed architecture, not a promise
that an existing project's core can be copied into Rust without substantial work.
