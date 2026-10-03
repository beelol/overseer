# AC201 focused setting-aware qualification

The compiler/test slot was released after the commands below completed. No UI, package, full suite, wider settings matrix or native-cause test was run in this slot.

| Source | Channel | Check-ins | Actual cases | Result |
| --- | --- | --- | --- | --- |
| frozen `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2` | off | off | AC189 + AC190 | 0 passed, 2 failed, 99.37 s |
| PR66 `2566e833cb3a9f9d7e65ed3338e3751aa42d0fbb` | off | off | AC189 + AC190 | 2 passed, 0 failed, 105.35 s |
| same PR66 | on | every:3 | AC189 + AC190 | 2 passed, 0 failed, 93.84 s |

The baseline failures are fixture expectation mismatches, not daemon setup errors: AC189 waited for a finished check-in despite cadence off; AC190 expected a briefing despite channel off and received the unchanged owner's prompt while the companion was running.

Corrected AC189 off phase observed seven completed turns, an actual `going_in_circles` free check, and no check-in start/result. It then explicitly enabled every:3 and retained all original positive assertions. Corrected AC190 off phase held a genuine working companion while a second actual channel fixture observed no MCP channel, no briefing/report/ask/claim effects, and the owner's exact prompt. It explicitly enabled the channel for new agents and retained the original positives. On receipts prove effective on/every:3 before the existing intentional overrides (AC190 explicitly turns cadence off). These are two focused tests across two profiles; they do not establish the full AC201 Verify clause.

Baseline execution used the already-built `overseer-d38fcd7c7660309e` read-only in the root integration target. `baseline-overseer.d` names frozen source `/private/tmp/overseer-closeout-next-20261003/daemon`; `baseline-provenance.txt` records source, test/daemon SHA256 and mtime. No build or mutation was performed in that source/target.

PR66 was rebuilt with `nice -n 20`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, only `cargo test -p overseerd --test overseer --no-run --message-format=json`, using `/private/tmp/overseer-closeout-verify-target`. Cargo reported the artifact `fresh: false`. `pr66-provenance.txt` and `pr66-overseer.d` identify the actual settings clone and daemon binary; `pr66-tests.txt` lists exactly the two intended tests. Each profile ran that verified binary with both exact names, `--nocapture --test-threads=2` and explicit profile environment. The fixture's heavy lock serializes these cases.

Logs and metadata are alongside this file. The tested PR66 source branch stayed clean and unchanged through both profiles. This evidence-only follow-up publishes these results; the source and test code remain identical to `2566e83`.
