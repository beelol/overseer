# Combined full regression at 2a1bd2a

Exact source: `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2`, isolated `/private/tmp/overseer-closeout-next-20261003`. Command: `nice -n 20 env CARGO_TARGET_DIR=/private/tmp/overseer-closeout-integration-target CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 scripts/test-all --jobs=1`. Session39632 terminated exit1; machine lock and final leftovers check completed normally.

Result: **86/88 stages**, not a full pass. Workspace Rust1601/1601, phone model152, phone app477, dev14/14, guided7/7, deploy6/6, source/link checks, VSIX packaging and all packaged UI scenarios except audit passed. Voice258s, talk30s, voice-turn-on36s and queue-pause35s passed. Optional paid/performance/on-screen scenarios were not run; the log lists exclusions.

Failures: Unit27/28 because line-diff reached its assertions then hung at Node24 shutdown; root sampled and terminated only that test child, retaining the failure. Reviewed PR67 fixes natural process exit. UI audit grid text was1017 versus unchanged993 budget because two Mods bookkeeping rows appeared; reviewed PR69 suppresses these from conversation rendering while retaining logs and inspection. Neither correction is present in this frozen source, so their focused passes do not turn this run green.

The runner buffers successful stage output and records aggregate results here. Literal AC229/230 assertion mapping is in docs/audits/2026-10-03-voice-modes-closure.md; this source includes reviewed PR52/61. Later PR64–73 require separate qualification. No owner profile, paid model or production deployment was used. Screenshots may include fixture account labels and are not copied into this aggregate evidence.
