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

## Outcome during implementation (2026-09-24)

Branch Diff was adopted as planned: its review stack is vendored under
`extension/branch-diff/` with its MIT LICENSE, and the modified files carry headers (see
`extension/NOTICE.md` and [AC-03](verification/AC-03.md)). Verified in the packaged UI:
external-worktree discovery (via the Git extension's `openRepository`), the staged/unstaged
cancellation case (Workspace Dirty view plus daemon status), and live refresh within the
RFC bounds including a deliberately missed watcher event. One upstream behaviour needed a
change for agent workloads: under continuous writes the comparison restarted on every
change and never published; it now publishes after one restart and catches up.
Agetor, Parallel Code, Pane (AGPL) and XCB were inspected at pinned commits and skipped.

## Happy, for the phone remote (AC-115, 2026-09-26)

[Happy](https://github.com/slopus/happy) is a phone and web client for Claude Code and Codex,
MIT licensed. It was inspected at revision `8517ab232528a6046271d6010aaed663e1187dfc`
(2026-09-22), read only: nothing was built, installed or run.

**Decision: nothing of Happy is adopted as code.** No license text or notice is owed.

| What Happy has | Where | Why Overseer does not take it |
| --- | --- | --- |
| A server every message passes through (`https://api.cluster-fluster.com`, or a self-hosted one), reached with socket.io | `packages/happy-server`, `packages/happy-app/sources/sync/apiSocket.ts`, `serverConfig.ts`, `packages/happy-cli/src/configuration.ts` | The owner's rule for this gate: the phone talks to Overseer's daemon only, with no relay, no server and no account. No direct mode on the local network was found in the app or the CLI. |
| Accounts: a challenge signed by the phone, tokens, a QR code that links a terminal to an account (`happy://terminal?<public key>`), a backup of the secret key | `packages/happy-app/sources/auth/`, `packages/happy-cli/src/ui/auth.ts` | Overseer pairs a phone with one Mac and has no account. The pairing code carries the Mac's key, a secret valid once, and its addresses. |
| End-to-end encryption through the relay: libsodium `crypto_box` and `crypto_secretbox`, AES-256-GCM for content keys | `packages/happy-app/sources/encryption/`, `sources/sync/encryption/`, `packages/happy-cli/src/api/encryption.ts` | It protects stored messages from the server. Overseer needs a live session authenticated in both directions with forward secrecy (Noise IK, AC-118). Taking Happy's would add a second design of security code, not remove one. |
| Its own wrapper around Claude Code and Codex, with its own message schema | `packages/happy-cli`, `packages/happy-wire` | Overseer's daemon already runs the agents and has its protocol (`protocol/protocol.json`). |
| The app: Expo 55, React Native 0.83, FlashList 2, Reanimated 4, its own Markdown parser, MMKV, Unistyles, LiveKit, RevenueCat, PostHog | `packages/happy-app/package.json`, `sources/components/markdown/` | The screens are bound to its sync layer and its accounts. Overseer's conversation must read like VS Code's, so its view models are ported from the extension instead (`phone/model`), with parity tests. |

What the inspection did confirm: a shipping app of the same kind holds long conversations on
the stack chosen for this gate (Expo, the New Architecture, FlashList, Reanimated), and parses
Markdown itself instead of using a web view. Both match the choices in
[the phone remote RFC](rfcs/phone-remote.md).

For the later relay RFC, two parts are worth reading again: `packages/happy-server-self-host`
(a relay one person can run) and `packages/expo-tailcat` (an app-scoped WireGuard connection
with no system VPN; experimental, with its own `THIRD_PARTY_NOTICES.md`).
