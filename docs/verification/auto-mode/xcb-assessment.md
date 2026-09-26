# XCB reuse assessment — current local decision

Inspected source: [`hraness/xcb` at `9b3590241f1faf15d669ad46bfde65467fda14d2`](https://github.com/hraness/xcb/tree/9b3590241f1faf15d669ad46bfde65467fda14d2), version 0.8.10. Its workspace declares MIT and Rust 1.97.1. Overseer's installed build toolchain is Rust 1.89.0; `rustup toolchain list` shows no 1.97.1 toolchain. This is a real integration constraint, not a reason to skip the RFC's reuse gate. No XCB source has been copied into Overseer; focused upstream runtime tests ran with a temporary toolchain, as recorded below.

## Relevant components

| XCB component | Observed source behavior | Overseer fit / gap |
| --- | --- | --- |
| `xcb-core/src/usage.rs` | Typed counters/quota points, freshness and account-wide block windows, uncertainty helpers, unit tests. | Promising pure logic. The Rust version floor needs a deliberate toolchain or code-port decision. Its selected account-wide windows do not cover arbitrary model-specific Codex buckets. |
| `xcb-runtime/src/codex.rs` | Metadata-only `account/rateLimits/read` and normalized quota parsing. | Valuable collector design; Overseer already owns isolated profiles and app-server process state, so a second account owner is risky. Prefer adapter/codec reuse or a read-only bridge first. |
| `xcb-runtime/src/claude.rs` | Structured `rate_limit_event` parsing with fixtures. | Valuable parser design; currently installed Claude Code is 2.1.246, while newer documented status-line rate limits require a later version. Verify the installed event stream with a signed-in owner account before claiming live coverage. |
| `xcb-runtime/src/routing.rs` and `task_classifier.rs` | Model/effort ranking and bounded capability classification. | Relative cost/quality numbers are hand-authored heuristics; classifier predicts an operator's past choices, not measured task quality or subscription draw. Cannot directly satisfy AUTO-AC-32/33/36. |
| `docs/route.md` | One-turn JSON route with account custody, settled outcomes, and dry-run preview. | Caller owns repeated delegation, which fits Overseer's coordinator concept. Full runtime also wants custody and process/workspace ownership; integrating it unchanged could create two authorities. OpenCode route support is not established. |

## Candidate integration order

1. Inspect and test typed, read-only quota parsers and model discovery behind Overseer's current account and supervisor ownership. A pinned fork or source port could bridge the Rust version gap; preserve MIT notices and upstream revision.
2. Test a bounded metadata-only bridge if parser reuse is impractical. It must not sign in to accounts, copy credential stores, take an execution lock, or start a coding turn merely to inspect usage.
3. Consider full `xcb route` only after proving custody, tool/permission coverage, context handoff, workspace confinement, cancellation, and restart semantics fit Overseer. This option is not presumed to work.

The narrow native Codex allowance collector now provides a tested local path while XCB adoption remains open. Passing XCB's own tests is necessary for adoption but cannot establish the Overseer bridge. No fork has been created.

A later Claude native-event slice inspected XCB 0.8.10's supported single-window and `unifiedWindows` event shapes and wrote an independent, strict Overseer normalizer. Its three daemon fixture scenarios pass, but this is not XCB integration or a fork. It does not satisfy the remaining custody bridge, toolchain, packaging, or rollback gates.

## Focused upstream test evidence (2026-09-25)

The pinned source was tested from a read-only clone using a **temporary** Rust 1.97.1 toolchain and dependency cache under `/private/tmp`; Overseer's Rust 1.89.0 installation and `Cargo.toml` were unchanged. `xcb-core usage` passed 3 unit and 1 contract test. `xcb-runtime codex` passed 40 matching unit tests and 10 matching authentication tests. `xcb-runtime claude` passed 7 matching unit tests and 7 matching integration tests. No model turn or paid probe ran. The first attempt with Rust 1.89.0 failed at XCB's declared rust-version gate, as expected. These are upstream tests only; no Overseer integration or full XCB suite has run.

The installed Codex 0.155.0-alpha.16.4 app-server accepted `initialize` followed by `account/rateLimits/read` in a read-only metadata session with API-key environment variables removed. Its response contained `rateLimitsByLimitId` with two buckets and the expected primary/secondary fields. The probe did not start a thread or model turn and recorded only field names and bucket count, not account identifiers or balances. This confirms the metadata source exists on this host, but does not prove an Overseer route can safely share account custody with XCB's runtime.

**Current integration ruling:** do not embed the full XCB runtime yet. It requires a newer compiler and owns account/session/process boundaries that Overseer already owns. Its tested, MIT-licensed quota codecs remain candidates for a pinned, narrow source port or bridge. The cost/quality routing profiles still cannot be treated as measured subscription consumption. Cost of this ruling if wrong: a narrow port may duplicate maintenance that a safe full bridge could have avoided.

## Next evidence to collect

- Run the full relevant XCB suite and test a narrow Overseer adapter boundary before deciding whether to port or depend on any component.
- Prototype a no-credential, metadata-only boundary against one Overseer isolated account; compare results with native harness output without a paid prompt.
- Test OpenCode, permission prompts, and worktree ownership through a controlled provider before deciding whether full runtime reuse is viable.
- Freeze any retained fork revision and patch set; verify packaging, notices, upgrade, and rollback.
