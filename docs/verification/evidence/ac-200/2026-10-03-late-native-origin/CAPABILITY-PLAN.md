# Narrow AC200 native turn capability correction

Baseline 11bd1c1 proved one interrupted genuine finding request, already received before real native cancellation, became an OPEN owner-cause archive Confirm proposal after a real owner successor. No Yes/archive effect. Exact RED evidence pushed at 3780a3d. Current production/test changes below are authored, UNCOMPILED and UNRUN.

## Authenticated identity and publication

- store.rs migrates overseer_tokens with nullable native_turn_id/revoked_ms. Existing unbound direct/agent/watcher rows stay compatible for reads; missing native turn identity never supplies owner authority.
- mod.rs native_launch_token validates actual turns/run_roles, revokes prior bound native capabilities and inserts an opaque capability hash bound to exact run+turn in one transaction. Retry of the same durable turn receives a new capability; failed launch never unrevokes a predecessor.
- daemon.rs start_turn_internal has already persisted the actual Turn before its launch closure. For native Overseer role only, it builds immutable per-launch MCP config/capability before adapters::launch/spawn_process. Neither arbitrary model arguments nor current mutable process generation determine origin. Generation is a private file layout label only.
- session.rs creates a unique directory under the actual run's process directory. Capability and config files use exclusive create_new with 0600 at creation. Claude keeps strict MCP/allowlist/denials; Codex keeps read-only and per-tool approval overrides; OpenCode uses process-scoped OPENCODE_CONFIG_CONTENT with the existing read-only tools map. Config and argv contain only a private capability-file path, never its token value. Initial ensure_overseer_run stops issuing/remapping a pending run-wide token.
- mcp.rs reads --capability-file once at startup using O_NOFOLLOW and verifies the opened regular file's UID/private mode. A missing/invalid file never falls back to OVERSEER_MCP_TOKEN. The existing environment route stays for ordinary agents/watchers/direct compatibility.
- Protected native propose/answer keep PR76's real TURN_START wait (no Store held). Postwait token lookup preserves actual run/role telemetry. capture_native_origin resolves only that token's durable bound turn plus its actual active session, with no latest-turn substitution or mutable last_cause fallback. Final proposal INSERT rechecks token existence, binding, revocation, exact latest turn/cause and unchanged active session in the same Store critical section. Guards are dropped before action effects.

## Resume, Continuity and migration

Native follow_up_via_stdin supports only generic. Claude/Codex/OpenCode successor turns already launch resumed new processes, receiving distinct capabilities and private configs. Old MCP processes retain their read-once capability; it is revoked at the next native launch. Revoked/deleted capabilities refuse; Fresh still applies the independently verified session boundary.

channel.rs carry_role defers only Overseer config creation to actual durable-turn launch, keeping role/session moves, read-only mode and predecessor token deletion. Ordinary watcher/agent routes remain unchanged. Direct Continuity starts currently do not publish an overseer_turns cause row; protected calls therefore retain a prior explicit provenance gap and refuse. No owner cause is invented. Existing Continuity compatibility evidence must be described as successor read-tool/read-only compatibility, not protected action qualification.

Legacy Claude migration accepts only scratch/mcp.json or the actual stored workspace/mcp.json, exact daemon command/socket shape and stored unbound Overseer token for that run. It removes only the exact contiguous daemon-generated config/strict/allow/deny argument group. Unknown/overwritten/revoked layouts or ambiguous groups refuse visibly. Codex likewise qualifies its exact generated group and credential; unrelated arguments survive. No owner configuration is read.

## Authored verification

- Original real interrupted-predecessor test retains actual finding, receipt, structured interrupt, MCP closure, shim exit, same session/run, owner successor, unique marker/cursor and no effect assertions. Its baseline same-token assertion becomes explicit rotation and predecessor revocation. Baseline source/evidence remain immutable.
- Existing two initial-publication wire tests retain real session-bind/origin publication windows and actual contested TURN_START acknowledgment; capability now proves actual run+turn binding before launch, without a pending-token rewrite.
- New capability fixtures: restart retains exact original turn and valid owner native proposal; unbound direct token reads remain compatible but protected proposal refuses; injected explicit revocation refuses with caller telemetry; one actual failed successor launch retains predecessor revocation; exact legacy scratch migration keeps an unrelated argument and only one immutable config; missing/public/malformed capability files never use a valid environment fallback.
- Two pure argument-group cases cover scratch and Continuity-shaped paths/policy neighbors, and ambiguity leaving saved arguments intact. Runtime layout/content/token qualification is separate from these parser assertions.
- Existing native origin, stale-session, finding/owner/global pending-answer, refusal-redaction and appropriate continuity/role controls will be run only in a coordinated bounded slot.

JavaScript parser and diff checks pass. No Cargo/UI/model/provider/owner-profile checks have run for the authored fix. Fixture configuration proves source wiring, not installed Claude/Codex/OpenCode runtime behavior. Full combined qualification and broad AC200 remain pending.
