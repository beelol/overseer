# Exact historical native tool-list compatibility (UNRUN)

Base: 7d30ec6b5603ea526c2937abded6fa9e836cbf3e, containing frozen PR78. Separate branch `codex/native-legacy-lists`; PR78 is unchanged. No production change, Cargo, UI, installed harness or model run at this checkpoint.

## Historical inputs

The independently read committed `tool_list("overseer")` has two exact generated epochs. Source blob and SHA-256 references are in `historical-source-proof.json`.

- Gate S 4c371ecd1f57affd6b5328a3ba9cf4eba2ee82b6, unchanged through 24c3c245bb8e8ac4df7ccf2373fc9728de792135: `roster,agent,conflicts,conversation,changes,diff,file,search,usage,check_in,rally,answer,propose` (13).
- 3ab9c1f738a5a177f46f79e7fa4bf03daac35618 adds `accounts` between usage/check_in, unchanged through 55ebf1d22760280ba750007c9d49cd315ddda919: `roster,agent,conflicts,conversation,changes,diff,file,search,usage,accounts,check_in,rally,answer,propose` (14).
- Queued Mods reads 116c9cb8c5ebfe02dfdcd05c7003e69a6ded1f7b inserts `mods` immediately after roster, producing 15. This commit is inspected in the Mods clone, not merged into this checkpoint.

Both historical `session.rs::tools_launch` implementations generate Claude's contiguous 7-argument group: `--mcp-config <daemon path> --strict-mcp-config --allowedTools <exact comma-joined MCP-prefixed list> --disallowedTools <fixed denial list>`. Overseer's wrapper explicitly passes read_only=true. Codex generates exact `-c` command/args/env pairs followed by one ordered `-c mcp_servers.overseer.tools.<name>.approval_mode="approve"` pair for every listed tool. Read-only mode is separate launch metadata and must survive migration. Continuity `carry_role` reuses `tools_launch`; its workspace/mcp.json layout remains subject to existing full config, real unbound token and path qualification.

## Narrow correction after actual RED

Current PR78 migration computes the expected saved group from the live tool catalog. That is wrong for a real saved 13-tool group now, and a saved 14-tool group after Mods reads enters the candidate.

Replace only the historical group recognition in `session.rs::without_legacy_overseer_args`: recognize complete, ordered, literal 13 and 14 vectors as named historical epochs (plus the exact current generated group if different). Never accept arbitrary subsets, permutations, unknown tools or missing pairs. Removing a recognized old generated group allows the existing launch code to produce the new private current-catalog configuration; the old list does not grant additional native authority. Do not persist an old run-wide credential or add an authority fallback.

Preserve all existing admission checks: actual known daemon layout, exact entire generated Claude JSON, matching actual unbound/nonrevoked Overseer run token, exact command/socket, contiguous group, remaining duplicate policy/config flags including inline forms, and Codex remaining server overrides. No user-added server/env/policy is dropped. No modified denial/mode relaxation. Ordinary agent/watcher routes remain unchanged.

## Authored real-boundary fixtures

`daemon/tests/overseer_native_legacy_lists.rs` contains literal vectors independent of current tools. Four named positives launch a genuine synthetic native Overseer, inject only the historic stored group/config with the actual run's disposable unbound compatibility token, and make one actual `run.follow_up` attempt. They assert same session/run, one replacement private config, no plaintext old token in actual argv, unrelated option retained, strict/deny or read-only controls, capability rotation and predecessor revocation. These qualify launch/config compatibility only: direct follow_up does not create an Overseer cause row, so protected Continuity propose/answer is still not claimed.

Two negative fixtures each exercise unknown, reordered and removed tool vectors. They require visible refusal before process/capability creation and preservation of saved arguments/config. Existing PR78 modified full-config and neighboring duplicate-policy fixtures must run as compatibility controls; no weakening of those expectations.

## Scheduled evidence plan (NOT RUN)

1. Source-fresh daemon build plus test no-run in coordinator-allocated target, nice20/jobs1/threads1; record source, executable, .d and exact test count.
2. Exact13-tool positives on unchanged7d30ec6: expected real compatibility RED. Exact14-tool positives are existing compatibility controls. Classify setup failures separately; no production mutation until genuine failure.
3. Reviewed minimal historical recognition fix, four positives + two negatives; existing modified-config/neighbor-policy controls and parser units.
4. When root integrates the finished Mods read slice, rerun identical frozen13/14 vectors against the 15-tool catalog and assert actual new private configuration includes `mods`. No fixture expectation recomputed from current list for historic inputs.
5. Combined full-suite gate belongs to root. No all-history/installed native/owner-profile/full AC200 claim.
