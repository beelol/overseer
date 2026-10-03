# CI fixture ordering evidence

Own test checkpoint: `05ed59d70caa024bed42e4ce161b864a9cee4e19`. Normal PR52 dependency merge: `2ee0b71e302d540cdf8cf0bc45f3de6864f1f53d`, retaining capture source `2bb6b1e6130249db45e387906b6bd275e1405f61`. The following commit adds the fixture-only actual-sampler observer and exact generated-context tag assertion. PR53 stays frozen at `f02665a9a273038bc18b7b8b0148d4c7de2f3043`; no queue behavior is changed here.

## Scope and observed ordering

AC140 holds the synthetic HTTP load response until real daemon memory samples are acknowledged. Acknowledgements are emitted immediately after `sys::memory` returns, only with both existing `OVERSEER_TEST_MEMORY` and explicit `OVERSEER_TEST_LOAD_SAMPLES`. No production sampling interval or policy changes. The fixture gate is bounded and waits without holding fixture-state locks; unload cancels the pending response before it can reinstall the fake model. The test retains refusal checks and proves two reads, a lower safe-memory sample, exact returned sample count/lowest memory, cancellation/unload for low and critical memory, and empty loaded inventory.

Shared launch preserves five isolated one-point meter samples at exactly 3000 with run identities. The sixth completed priced-start intentionally moves the meter by zero, yielding the existing 2000 conservative error-bound sample. The test proves that sixth observation's identity/readings and all six provenance entries, exact mean and sample deviation, and the unchanged conservative 4059 booking. Overlapping holding parents are held by a bounded fixture gate so their excluded provenance is deterministic.

AC193 prepares named watchers sequentially for the existing broad watch test. A separate gate keeps an actual queued companion briefing active after the first completion and proves the existing busy refusal, exact queued bytes/source delivered once, then accepted watch after actual idle. Briefings remain enabled.

AC190 and AC198 reuse PR52's capture gate. Each deliberately observes raw harness completion while the prior captured reply remains visible, then releases capture and waits for a new reply identity plus actual Map/owner answer content. Original Rally two-agent report/cost/area checks and quiet/burst/cap/usage/guardrail checks remain. These are fixture proofs, not live owner evidence or a complete criterion verification.

## Commands and results

Every Rust command used `CARGO_TARGET_DIR=/private/tmp/overseer-ci-fixture-ordering-target CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 nice -n 20` in the isolated worktree.

| Command after `cargo test -p overseerd` | Result | Evidence |
| --- | --- | --- |
| `--test continuity ac140_no_model_over_the_budget_is_loaded_by_any_path -- --exact --nocapture` before observer | Expected failure: missing two actual-read acknowledgements, exit101 | [red](ac140-red.log) |
| Same, first observer attempt | Fixture assertion failed because the context-created load uses `overseer/qwen2.5-coder-14b:16k`, not the base tag; test corrected to assert the real outgoing model and wait for that exact tag | [first attempt](ac140-first-green-attempt.log) |
| Same, corrected exact context tag | 1/1 passed, 1.27s | [green](ac140-green.log) |
| `--test shared_launch qualified_draw_prices_a_booked_start_after_five_isolated_runs -- --exact --nocapture` | 1/1 passed, 9.46s | [draw](shared-launch-green.log) |
| `--test overseer ac193 -- --nocapture` | 2/2 passed, 100.73s | [watch](ac193-green.log) |
| `--test overseer ac190_rally_asks_only_where_the_digests_cannot_answer -- --exact --nocapture` | 1/1 passed, 10.91s | [Rally](ac190-green.log) |
| `--test overseer ac198_quiet_and_bounded -- --exact --nocapture` | 1/1 passed, 31.86s | [quiet](ac198-green.log) |
| `--test continuity -- --nocapture` | 13/13 passed, 5.40s | [compatibility](continuity-green.log) |

`node --check` passed for both modified Claude and Codex fixtures. `git diff --check` passed. All own Cargo sessions finished; the process scan found no own-target daemon or fixture process. Compiler slot released to Mods. No UI, paid model call, production-daemon action, or full suite was run in this slice. Full integrated verification remains the coordinator's gate; no criteria or ledger status is advanced here.
