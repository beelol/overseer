# Per-launch native capability results

Final reviewed production source: `1fc08886fb335002f32f059cedd888c05eb0582a`. Based on integrated frozen `55ebf1d22760280ba750007c9d49cd315ddda919`. Evidence-only follow-up does not change that source. The coordinator independently reviewed source and actual logs. This is a bounded AC200 correction; broad AC200 and combined full qualification remain pending.

The original TRUE authority RED at `11bd1c1` is preserved in baseline-wire.log: a genuinely received finding request survived real interrupt/MCP closure/shim termination, then created an OPEN archive Confirm proposal with a later real owner turn's cause. No Yes/archive effect occurred. The fix gives each actual native launch a distinct private read-once capability bound to the durable run+turn, revokes predecessor capabilities when superseded, and revalidates capability/session/origin under the proposal insertion lock. Initial native calls still wait for actual TURN_START publication; no owner fallback or model-supplied origin is accepted. Agent/watcher tokens and owner/voice wrapper semantics stay unchanged.

## Exact runtime phases

| Phase/source | Actual result | Evidence |
| --- | --- | --- |
| Original unchanged authority baseline `11bd1c1` | Intended authority RED 0/1, 8.49s; actual OPEN owner-cause archive Confirm, no effect | baseline-wire.log and baseline build/dependency/source proof |
| Capability production `2e7d104` | Interrupted predecessor GREEN 1/1, 8.57s; revoked refusal, no proposal | fix/late-green.log |
| Capability production `2e7d104` | Initial publication wire controls 2/2, 2.72s; both actual contested TURN_START acknowledgments and legitimate owner proposals | fix/publication-green.log |
| Initial lifecycle fixture at `2e7d104` | 5/6; intended capability assertion not reached in one case because test expected failed run rather than the failed durable new turn | fix/capability-fixture-assumption-failure.txt |
| Test-only correction `18aef68`, unchanged production `2e7d104` | Failed-launch exact case 1/1, 2.31s; six lifecycle cases 6/6, 7.16s | fix/failure-case-green.log, fix/capabilities-final-green.log |
| Unchanged capability production `2e7d104` | Native origin/current owner/finding/global pending-answer/stale-session/missing-row controls 5/5, 33.37s | fix/native-controls-green.log |
| Unchanged capability production `2e7d104` | Actual native tool/MCP refusal redaction 2/2, 1.48s | fix/refusal-controls-green.log |
| Unchanged capability production `2e7d104` | Exact Continuity successor read-only/read-tool control 1/1, 2.03s | fix/continuity-read-green.log |
| Unchanged capability production `2e7d104` | Exact role entitlement 1/1, 1.00s; exact read-only/config control 1/1, 1.77s; two pure migration units 2/2 | fix/role-control-green.log, fix/read-only-control-green.log, fix/migration-units-green.log |
| Added ambiguity fixture `05e0314`, unchanged capability production | Intended migration refusal RED 0/1, 1.54s; an actual second native turn launched | ambiguity/red-wire.log |
| Final production `1fc0888` | Exact duplicate-policy GREEN 1/1, 1.49s; affected exact/modified config migration controls 2/2, 2.00s; three parser units 3/3 | ambiguity/green-wire.log, ambiguity/migration-controls-green.log, ambiguity/migration-units-green.log |

The lifecycle assertion correction uses one direct run.follow_up, checks the actual durable new Turn is failed with ended_ms, then checks two capabilities/one revoked, old protected refusal and no proposal. It avoids the conversation retry queue. The earlier 5/6 is a fixture-assumption failure, not product RED; full raw synthetic session envelope stays local and only its diagnostic excerpt is published. Later ambiguity baseline is a genuine admission/refusal expectation RED, not an installed-CLI precedence claim.

## Build and execution receipts

All builds/runs used `nice -n 20`, one Cargo job and one test thread, the exclusive retired `/private/tmp/overseer-closeout-verify-target`, and approved disposable socket/process test privileges. No UI/full suite, paid model/provider, owner profile, production daemon or private audio was used. No automatic repeat of a failed test occurred; distinct corrected cases were rerun after reviewed changes.

Standalone build at `2e7d104` took 22.52s; selected no-run took 25.80s. Artifact receipts, hashes, exact one-test list and .d manifest/daemon footers are in fix/. The corrected lifecycle binary rebuilt at `18aef68` in 0.98s; its compiler receipt is preserved, but its historical executable hash was not captured before a later test-only rebuild. The other first-phase receipts record executable hashes at capture time.

The ambiguity baseline at `05e0314` compiled its changed fixture (fresh=false) in 0.48s; unchanged standalone daemon was fresh=true from the earlier reviewed build. Exact one-test list, source/head, hash and .d proof are in ambiguity/. Final standalone build at `1fc0888` was fresh=false (6.66s), final no-run 9.97s. The unchanged ambiguity fixture was legitimately fresh=true with its preserved baseline hash, while final daemon/parser-unit executables were rebuilt. final-build-proof.json records those distinctions. Full Cargo JSON streams stay local; only relevant artifact/source receipts are published. Final authored source hashes are in final-source.sha256.

## Remaining qualification

- Broad AC200 Verify and combined full suite remain pending; unchanged controls were not rerun after the final migration-only guard. The coordinator owns the combined final-source gate.
- Installed Claude/Codex/OpenCode runtime and resumed-session interpretation are unqualified. Native wire/lifecycle fixtures prove actual daemon/bridge behavior with synthetic harnesses. Codex/OpenCode launch wiring is source/fixture compatibility, not provider qualification.
- Continuity starts through start_turn do not currently publish an exact overseer_turns cause row. Successor read-tool/read-only compatibility passed; protected native action qualification remains a prior explicit gap and fails closed. No predecessor/owner cause is invented.
- Legacy migration qualifies an exact current daemon-generated allow-list. It does not claim arbitrary historical lists remain compatible when tool_list changes (for example, a later Mods read tool). Known frozen historical shapes need separate bounded qualification before such a combined merge; unknown/subset authority is not accepted.
- Same-process/token misuse outside the proved superseded-launch path is not a blanket solved claim. Protected tools require their actual source-generated capability and exact durable origin; missing/revoked/unbound provenance refuses. Native lag/restart behavior outside these fixtures is unchanged/unqualified.

Source/whitespace and JavaScript parser checks passed. Raw libtest logs intentionally preserve their EOF blank lines; diff --check reports those evidence-only trailing blank-line warnings, while source/docs checks excluding raw logs pass. This branch overlaps orchestrator turn-launch/session/channel/token handling and their fixture readers; no criteria/ledger/main changes are included.
