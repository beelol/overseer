# AC200 tool refusal redaction — 2026-10-03

Implementation baseline: PR54 `c22da6fc`. Separate branch `codex/tool-refusal-redaction`; PR54 remains frozen. This narrow patch resolves the independently found model-visible refusal boundary, not the full AC200 Verify clause or historical stored data.

A real isolated daemon returned a synthetic credential verbatim from an invalid claim path. An actual MCP shim also returned an unsanitized synthetic error from an isolated Unix-socket peer. `red.log` records both failures (2 regression failures, 4 shared-helper passes). No owner credential, login, production daemon or paid harness was used.

The public `overseer_tool` boundary now redacts and bounds result text and propagated error messages, preserving result `is_error` versus JSON-RPC failure. Successful normal tools retain the prior 32 KiB/redaction behavior; early native Swarm returns also pass through this boundary. MCP independently sanitizes its fallback error text. The new fixture checks claim, invalid action and invalid file-path errors through daemon RPC and actual `tools/call` wire responses; forged-token refusal stays a daemon error. A synthetic socket peer proves the shim fallback independently of daemon sanitization.

Child stdout and exit waits are bounded; timeout and unwind paths kill/reap the MCP child. The peer has an accept deadline, read/write timeouts and a scoped join. Test daemons and generic runs are isolated and cleaned up by the existing helper.

Exact focused commands used `CARGO_TARGET_DIR=/private/tmp/overseer-closeout-pr49-target`, `nice -n 20`, `--jobs 2`, and `--test-threads=2`:

- `cargo test -p overseerd --test tool_refusal_redaction`: red 2 failures + 4 helper passes.
- `cargo test -p overseerd --test tool_refusal_redaction --test overseer_redaction`: green 6/6 (2 new regressions + 4 helpers) and 9/9 (5 existing PR54 regressions + 4 helpers), recorded in `green.log`.
- `cargo test -p overseerd --test overseer ac180_mcp_shim_serves_overseers_tools_from_the_daemon`: green 1/1 actual MCP success/role check, recorded in `mcp-green.log`.
- `git diff --check`: passed.

The parent independently reviewed the two production files and tests without builds, first requesting bounded fixture cleanup and then accepting the final source and 6/9/1 focused checks. Full combined verification remains the coordinator's gate; no UI or full suite was run for this patch. Unreproduced attribution/settlement/file-boundary candidates from the broader independent review were not changed here.
