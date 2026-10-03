# Native turn origin: bounded AC200 correction

Source fix: `eee9aba02c4d3d607944515d6e2f975d11d4d9da`, based on frozen integration `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2`. Tests ran against the unchanged source/test tree committed as that fix. AC200 remains partial; this is not its full security qualification.

## Reproduction and impact

`red.log` is a genuine authority assertion failure, not a setup error: one native finding remained active with the same session/run/durable finding turn. A real unrelated agent awaited permission. The owner typed `handle what needs me`, which surfaced that actual question without another Overseer turn. This shortcut changed the mutable session cause to owner. The finding's actual gated native archive call then returned success and created an OPEN owner-caused archive proposal. Archive still required explicit yes; none was approved or executed. The unrelated permission was declined for cleanup before the failing assertion. Baseline production was unchanged2a, with only the accepted regression added. Result: 0/1, 9.18 s.

`missing-red.log` is a separately labeled injected-state assertion failure, not a reproduced initial launch race. The fixture loaded an actual current native owner run/turn, held its MCP call, and deleted only that turn's durable origin row while preserving session/run/token and owner-valued last_cause. Baseline incorrectly created an archive proposal instead of refusing. Result: 0/1, 2.59 s.

## Correction and focused verification

The native proposal/answer helper resolves the caller's actual latest durable turn and its exact session-bound cause under the existing Store lock. It passes captured immutable turn context through handling, never the mutable session shortcut cause. Missing provenance explicitly refuses. The final proposal insertion lock revalidates the same session/run/turn/cause; a successor transition during handling cannot substitute authority. Owner/voice direct wrappers and global pending-question eligibility are unchanged. No launch/admission plumbing, protocol schema, role/tool/class/level/cap/reach changes or question-session ownership rule was added.

| Check | Actual result |
| --- | --- |
| Native caller file | 9/9: 5 specific cases plus 4 existing helpers; 18.62 s |
| Existing redaction file | 9/9: 5 specific cases plus 4 helpers; 3.35 s |
| Existing refusal file | 6/6: 2 specific cases plus 4 helpers; 1.63 s |
| Exact AC230 simulated voice case | 1/1; 18.37 s |

The native file preserves valid current owner Confirm requests, finding refusal/quiet Steer, current native answers to still-pending global asks, and PR63's archived finding/answer refusals. The two new cases now refuse correctly. The voice case retains actual native mode/Auto requests with explicit yes and unsolicited permission readback. Logs are complete, including the existing 27 compiler warnings. Source `git diff --check` passed; raw cargo logs retain terminal blank lines, which can trigger evidence-only EOF whitespace warnings. Independent source review was accepted by the coordinator before compatibility completion. No test result was inferred from a zero-test run.

Commands used `nice -n 20`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, and exclusive `/private/tmp/overseer-closeout-verify-target` in isolated clone `/private/tmp/overseer-ac200-native-turn-20261003`. The original baseline test artifact's manifest and exact one-test list were confirmed before execution (`red-build-proof.json`); the baseline daemon artifact was warm, while its regression test artifact was rebuilt. Full Cargo metadata stays in local scratch, not this evidence. The fixed native file and compatibility commands visibly rebuilt/ran their named binaries and nonzero cases. All daemons were disposable fixtures; no UI, microphone capture, owner profile, paid call or production daemon was used. Compiler/test slot was released immediately after the final focused case.

## Limits retained

- Initial cause publication still follows native process launch. Calls before exact durable provenance exists now refuse safely; this slice does not eliminate a potentially premature refusal. The injected missing-row case tests the safe boundary only.
- A run token still lacks authenticated turn identity. A truly late predecessor transport that first reaches attribution after a successor has begun is not qualified by this fix. Ordinary awaited MCP calls cannot simply be assumed to overlap successor launch; no artificial orphan transport was used to claim that vulnerability.
- Broader AC200 Verify fixtures, the full security review/resolutions and final combined full-suite/runtime qualification remain required. There is no historical-session migration or complete AC200 claim.
