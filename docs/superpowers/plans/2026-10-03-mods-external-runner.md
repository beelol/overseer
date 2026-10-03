# Less tool noise: private external runner implementation plan

AC271–273 remain open. This implements the already-authorized optional bundle design; it is not a request to install RTK, enable a transformer or change a native harness. The earlier synthetic macOS probe and no-fork receipts are design evidence, not runner qualification. Main RFC Verify clauses remain authoritative.

## First implementation boundary

Add an internal Rust runner under daemon/src/mods, initially reachable only through isolated integration fixtures. Its input is a verified immutable executable reference, a typed allowlisted recipe, bounded raw bytes, a private turn scratch reference and cancellation/deadline controls. No arbitrary executable path, shell command, environment map or sandbox policy comes from a model or public RPC. Keep local text manifests unchanged until a separate confirmed code-install transaction is implemented.

A runner result distinguishes transformed bytes from a static bypass reason. Nonzero exit, timeout, cancellation, unsupported platform, invalid reference, excessive output, invalid text and unknown recipe all bypass to the unchanged original bytes retained by the caller. Never rerun the original tool command. A successful external process alone does not establish protected-fact preservation or native delivery; those are later gates.

## Required baseline fixtures before production code

Use a test-owned helper executable with deterministic modes; do not download or run RTK. Fixtures operate only on synthetic files, sockets and listeners. Freeze expected results independently of implementation helpers.

1. Successful bounded stdin/stdout round trip, including Unicode/newlines and empty output. Preserve exact original bytes and command status in the caller fixture.
2. Synthetic protected file/config/socket/other-run read/write attempts fail; scratch write succeeds; executable mutation and symlink escape fail. Unsandboxed controls prove the targets exist and are accessible. No owner path or production socket is probed.
3. Owned loopback TCP and Unix listeners receive no connection under policy; positive controls connect. Environment output contains only the runner allowlist and private HOME/TMPDIR, with no inherited credential or provider variables.
4. Fork/spawn attempts are refused under the shipped no-fork policy. Timeout and cancellation kill and reap the owned process group; no child/helper/listener remains. Keep separate evidence if process-group cleanup is tested with a diagnostic child fixture rather than the shipped policy.
5. Blocked stdin, endless stdout, endless stderr, simultaneous output, silent hang and cancellation all terminate within the whole-operation deadline. Bound bytes retained from each pipe; drain or terminate without deadlocking. Deadline includes process launch and all pipe/mutex waits, not just a final waitpid.
6. Changed executable bytes/reference, replaced/symlinked executable or scratch ancestors and unavailable OS enforcement refuse before program execution. No permissive fallback. Interrupted fixture cleanup leaves no mutable global files.

These cases first assert a missing runner boundary, with startup/setup failures classified separately from actual enforcement failures. Do not infer all hostile cases from one missing-function failure. Compiler/runtime execution requires the coordinator's exclusive slot.

## macOS execution design to qualify

Use deny-default OS enforcement with the executable/reference mounted logically read-only and only the exact per-turn scratch subtree writable. Generate path rules from canonical, validated private paths with correct policy-string escaping; reject embedded controls and alias/symlink races rather than interpolating unchecked strings. The probe's broad developer-runtime allowances are not automatically production requirements. Qualify the minimal native Rust runtime reads (System/usr-lib, literal ancestor metadata, root literal and entropy/null devices) with the actual helper before adopting them. Network, owner configuration, daemon sockets and other turns stay denied by default.

Clear inherited environment and set a small fixed PATH if required, private HOME/TMPDIR and explicit telemetry opt-out. Launch directly, without a shell. Retain an owned process handle/group and guarantee reap/cleanup on every return path. Platform enforcement failures report unsupported/bypass; no allow-default fallback. Do not claim Linux/Windows support until separately qualified.

Keep input/output and helper stderr private. Public events contain static reason codes, digests, byte counts and timing, never raw content or arbitrary helper error strings. Retain original stdout/stderr/status byte-for-byte under the existing scoped retrieval contract when that contract is implemented; a bounded runner input limit cannot silently truncate the original result.

## Subsequent mandatory gates

- Confirmed code install/update: a distinct typed manifest/program recipe; pinned source/artifact/dependencies/permissions; explicit owner confirmation bound to exact staged bytes; separate size budgets, interrupted rollback and no global installation. Imported code stays unavailable until this transaction is complete.
- Immutable turn references and removal: acquiring an executable reference prevents removal while active; removal disables new acquisitions, then cleans only owned files after references end. Crash/restart recovery must not invent live references or delete in-use artifacts.
- RTK qualification: independently verify pinned artifact/source integrity before execution, then evaluate only explicit candidate filters. Preserve warnings/errors/skips/protected facts; unknown, binary, small, larger or failed output bypasses. Do not infer safety or savings from shorter output.
- Harness adapters: actual installed-runtime/mock-provider next-input captures for Claude/OpenCode; one original command, no duplicate raw model body, hook conflicts/reconnect/children and scoped raw retrieval. Codex remains explicitly unqualified until a replacement boundary is demonstrated. Obey the existing prohibition on paid Claude turns.
- Accounting/evaluation: distinguish estimated payload reduction from reported tokens/cache/cost; include transformation and retrieval overhead and unknown fields. Frozen offline replay plus one-attempt gpt-5.6-luna low off/on comparisons and quality evidence remain required by AC273.

No AC closes on this plan, the earlier sandbox probe, a helper-only pass or a runner-only PR. The first runner slice must stay internal/unavailable until its callers and install/authority boundaries are qualified. All implementation goes in its own branch/PR; criteria and ledger updates go to main with exact source/evidence commits.
