# Less tool noise: pinned candidate and remaining implementation gates

Read-only research, 2026-10-03. AC-271–273 remain unverified. No transformer was downloaded as an executable, installed, run or enabled; no paid turn or owner profile was used. Source checkout inspected: combined follow-up `eeb1f3f`.

## Candidate pin

The [RTK release API](https://api.github.com/repos/rtk-ai/rtk/releases/latest) currently resolves to [v0.51.0](https://github.com/rtk-ai/rtk/releases/tag/v0.51.0), published 2026-10-02, target `e001f773f80b22b7dc4c7a79521b30e35aaef026`. The aarch64 macOS archive is 4,143,723 bytes; its published SHA256 is `8817d8b71afc02ac8bf06eb24bcc41c306592ab735b68e8fee9db1ba0de7cb59`. This is metadata, not independently verified artifact integrity or selection approval.

Pinned [pipe source](https://github.com/rtk-ai/rtk/blob/e001f773f80b22b7dc4c7a79521b30e35aaef026/src/cmds/system/pipe_cmd.rs) supports `pipe --filter cargo-test`, bounded UTF-8 input, explicit unknown-filter errors and raw fallback when filtering grows output. Its other filters include lossy search/diff handling; do not enable them implicitly. Automatic content detection is unsuitable for the proposed explicit allowlist.

The [entry point](https://github.com/rtk-ai/rtk/blob/e001f773f80b22b7dc4c7a79521b30e35aaef026/src/main.rs) calls a telemetry check before argument parsing. [Telemetry](https://github.com/rtk-ai/rtk/blob/e001f773f80b22b7dc4c7a79521b30e35aaef026/src/core/telemetry.rs) requires a compiled endpoint and explicit consent, and supports opt-out. Do not infer unconditional network activity. Nevertheless, the required runner must supply private configuration and enforce network denial independently of these application checks.

Source SHA256 receipts (temporary originals at `/private/tmp/overseer-transformer-readiness-20261003`): main `d683dd3ab2ba2c80daffb2b9ef36d0de4273a7df2692594fbe5206060f3628db`; pipe `bbc1e7c65beb03927365611434ca90afa2e16c6dd7e042a74702810a04c5621d`; telemetry `ce6915f0d8fa21509b2913d5822889a7865144d1fe94d1a4c5aa0008124e105a`; Cargo.toml `362b34c49755eddcf3f8d48bb80db8db6c2bb26f05a928efa2fd124684d5b29d`.

## Adapter boundary

[Claude hook documentation](https://code.claude.com/docs/en/hooks#posttooluse-decision-control) specifies schema-matching `updatedToolOutput` for built-in tools including Bash; malformed replacement falls back to the original. The installed binary contains that field, which proves presence only. Original telemetry is captured before this replacement. Actual next-model-input capture, resume, hook conflict and child behavior remain untested; no paid Claude qualification is permitted.

The [OpenCode plugin interface](https://github.com/anomalyco/opencode/blob/dev/packages/plugin/src/index.ts) exposes after-tool arguments and mutable output/title/metadata. This moving source is not installed-runtime qualification. Pin its resolved installed version before designing the shim. Codex replacement remains unsupported pending a suitable qualified boundary.

## Next implementation steps

1. Extend the existing strict manifest/library with a typed, pinned program recipe, separate code-change confirmation and staged artifact verification. Do not relax the text bundle size limit globally: the candidate archive already exceeds it. Keep unsupported imported recipes unavailable.
2. Implement and test an OS-enforced external runner before executing RTK: allowlisted environment, private home/scratch, read-only binary, denied network/owner files/sockets/other runs, bounded output/time and descendant cleanup. `/usr/bin/sandbox-exec` exists on this Mac; existence does not establish enforcement. Other platforms must qualify their runner or refuse code pieces.
3. Add immutable per-turn program references and deferred removal. The current text library removes files immediately; it cannot safely serve active program references unchanged.
4. Qualify one explicit successful-test filter using synthetic replay and protected-fact checks. Preserve raw stdout/stderr and status byte-for-byte under scoped retrieval. All failures, unknowns, larger outputs and unsupported transports bypass without rerunning commands.
5. Only then connect qualified result hooks and measure payload reduction separately from reported tokens, cache, overhead and quality. Preserve every full AC Verify clause; these steps are not a substitute for native/mock captures, hostile fixtures and the required off/on comparison.

## Synthetic runner startup probe

A deny-default sandbox profile could not launch even `/usr/bin/true` (SIGABRT, no diagnostic output). Adding process/sysctl/metadata access did not resolve it. An inert `/usr/bin/true` control with allow-default exited0, so the facility can launch a process but the restrictive runtime profile is not ready. No protected-read/network denial is claimed: those test children aborted before assertions. Files and scripts were synthetic under `/private/tmp/overseer-transformer-isolation-probe-20261003`; no owner paths or connections were attempted. The next runner task must diagnose required startup permissions and then prove narrow enforcement, rather than ship the permissive diagnostic profile.

## Restrictive startup and synthetic enforcement result

The follow-up [runner probe](../verification/evidence/ac-271/2026-10-03-runner-probe/README.md) resolves the startup failure: a literal-root read allowance, ancestor metadata and entropy-device reads permit direct Python execution under deny-default. Synthetic protected reads/writes, read-only program writes, symlink escapes and task-owned TCP/Unix connections are denied with EPERM while scratch writes work; unsandboxed controls prove the targets are accessible. A spawned child inherits the same restrictions. All processes/listeners were cleaned up. This is a design probe on macOS26.6.2, not an implemented transformer runner, timeout/descendant-cleanup qualification or completed AC. Remaining implementation steps above still apply.

A further synthetic no-fork profile removes the fork allowance and denies both direct fork and subprocess spawning with EPERM; positive controls succeed. See the same evidence directory's child-process denial section. This gives the runner design a concrete option for single-process transformers, subject to actual transformer/thread compatibility and timeout/output qualification. It is not a shipped policy or descendant-cleanup claim.
