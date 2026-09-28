# SWARM-17 — native descendant control

Status: partial. Implementation revision: `4b9b04b` on `codex/swarm-mode`.
Fixture/runtime support: local scripted harnesses only. No live Swarm target is qualified.

Input: a fixture target advertising `native_child_control` for Codex, plus a
synthetic Claude Code worker using the local fake harness. The latter writes
its launch file and emits a background-child stream without paid model use.
Gate S [AC-180](../AC-180.md) identified Claude Code's `Agent`/`Task` deny
setting as a candidate control; its read-only spike did not qualify Swarm.

Expected: caller-supplied target capabilities cannot authorize an unmanaged
native child. Reject a worker route lacking a daemon-owned launch restriction
before recording an attempt or quota reservation. For the Claude candidate,
persist the worker identity and include the native-delegation deny setting on
initial and resumed launches. A caller cannot override it with extra arguments.

Actual: `swarm.admit` returns `uncontrolled_native_delegation` for explicit
Codex, Codex app-server and OpenCode worker routes. The Codex fixture first
failed by admitting its forged capability and then passed with zero attempts
recorded. The adapter rejects those transports again at launch. A Claude
Swarm worker's launch includes `--disallowedTools Agent,Task`; the local
launch-file test confirms this and verifies that its private broker token was
not given to the synthetic provider. The adapter's initial/resume tests keep
the flag and reject extra arguments. Ordinary agent launches are unchanged.

At `8d9f328`, audit-only runs additionally hold the Claude candidate before
reservation because native-delegation denial does not enforce read-only source
access. The route-binding and synthetic-child fixtures use an explicit
isolated-write grant. This does not qualify Claude for live Swarm use.

Checks: `cargo test --offline -p overseerd --bin overseerd swarm_worker_ --quiet`
(3 passed); `cargo test --offline -p overseerd --test swarm_admission
--test swarm_runtime -- --test-threads=1` (32 + 19 passed). Daemon integration
tests ran with local Unix-socket/process permissions. `git diff --check` passed.
No paid or live account test was run.

Evidence: `daemon/src/adapters.rs`, `daemon/src/daemon.rs`,
`daemon/src/swarm/admission.rs`, and
`daemon/tests/swarm_runtime.rs` (`uncontrolled_native_delegation_blocks_admission_before_reserving_or_launching`,
`synthetic_claude_background_child_does_not_finish_swarm_attempt_at_launch_stub`).

Remaining: the fake Claude stream can ignore the deny setting, which is why
this fixture does not prove actual disablement. Observe a real attempted
unmanaged spawn and prove the selected installed version blocks it, or prove
bounded descendant admission/control, before enabling a live target. OpenCode
needs an enforceable profile policy; Codex and app-server need a qualified
control path or remain ineligible. Bind the qualification to installed harness
version, Auto's daemon-owned target identity, and normal Swarm launch. The
RFC checkbox remains open.

Follow-up (`cfa0eba8`, `claude/auto-swarm`): behind the undecided `swarm.native_director` switch, the proposed Claude director is launched through the normal Swarm start with `--disallowedTools Agent,Task` as well, and its launch is bound to a daemon version check (at least 2.1.246) and a `--help` flag check; native workers with Swarm tools keep the deny, and extra arguments cannot override it (`swarm_tools_give_claude_its_mcp_tools_and_deny_native_delegation`; `daemon/tests/swarm_native.rs`). This is still argument-level evidence on the synthetic fixture: no real Claude has been observed refusing a spawn, so the status is unchanged.
