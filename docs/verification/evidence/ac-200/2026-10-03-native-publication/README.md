# Native publication boundary: focused evidence

Source commit: `1f9892365f185d40a398c6dff7708fcc3b0537fb`, based on frozen PR72 `b2cfb93a4630c4decd95fc7cc2c72307aee74764`. Independent coordinator source review accepted the narrow synchronization and immutable-origin boundary before publication. This evidence is synthetic/disposable, with no paid turn, owner profile, private audio or production daemon.

A legitimate first native action could arrive before its token/session was bound or its exact durable turn origin was published. PR72 correctly refused missing provenance; these two real first-call cases demonstrate the resulting support gap. Native `propose` and `answer` now wait on the existing `TURN_START` mutex before definitive token lookup and immutable origin capture. The mutex and Store are released before effects; normal tool entitlement, actual-run telemetry and final session/run/turn/cause revalidation remain in place. Initial MCP initialization/tool listing remains ungated.

| Check | Result | Evidence |
| --- | --- | --- |
| Exact authored test listing and source/executable identity | 2 tests, both daemon/test freshly compiled from this isolated clone | `baseline-exact-list.txt`, `baseline-build-proof.json`, `baseline-dependency-proof.txt`, `baseline-source.sha256` |
| Baseline first native call before binding | True intended support RED: actual wire refused active-conversation binding; turn completed and gates released before assertion | `baseline-wire.log` |
| Baseline first native call before origin publication | True intended support RED: actual wire refused missing recorded turn origin | `baseline-wire.log` |
| Both publication cases after synchronization | 2/2, 2.35s; actual contested mutex acknowledgment, single successful wire reply, correctly attributed open Confirm proposal, no archive effect | `publication-green.log`, `green-source-proof.json` |
| Existing native origin/session/authority compatibility | 9/9, 17.82s (5 specific tests plus 4 shared helpers) | `native-refusal-compat.log` |
| Existing direct and wire refusal redaction | 6/6, 1.36s (2 specific tests plus 4 shared helpers) | `native-refusal-compat.log` |

The two-case baseline ran in 2.21s, 0/2 passed. Its actual native responses distinguish support RED from setup timeout. One initial build invocation mistakenly selected the nonexistent `overseer-daemon` package and exited before tests; `setup-wrong-package.stderr` preserves that setup error separately. The corrected package is `overseerd`. Existing compiler warnings are preserved in the green/compatibility logs. Four common helper tests were deliberately filtered from the two-case publication run. Source whitespace checks pass; the three unmodified raw logs retain terminal blank lines at EOF, reported by the evidence whitespace check.

Build and checks used `nice -n 20`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, and the exclusively coordinated `/private/tmp/overseer-closeout-verify-target`. Commands:

```sh
cargo test -p overseerd --test overseer_native_publication --no-run --message-format=json
# Verify generated executable --list contains exactly the two ac200_first_native_call cases.
<built-test-executable> ac200_first_native_call --nocapture --test-threads=2
cargo test -p overseerd --test overseer_native_publication ac200_first_native_call -- --nocapture --test-threads=2
cargo test -p overseerd --test overseer_native_cause --test tool_refusal_redaction -- --nocapture --test-threads=2
```

The baseline has identical inert publication hooks but no authority synchronization change. `baseline-scheduling.patch` reconstructs those hooks over `b2cfb93a`; its reconstructed source hash was checked against the observed baseline hash. Retrieve the unchanged test file from source commit `1f989236` before reproducing the baseline. Both scheduling stages and the actual contested-mutex observer are inert outside the existing NET fixture guard plus dedicated gate configuration. Fixture probes are explicitly off, requests use actual private launch configuration, and release-on-drop/socket deadlines prevent stranded launch peers. Published provenance excludes the full Cargo JSON and any token values.

This is partial AC200 evidence. A truly late earlier-turn native request using the same run token after a later native turn has started remains an identity/provenance gap. No token-per-turn design, broad security review completion, full suite, UI, owner/native model quality, paid/live transport or production qualification is claimed. Parent integration/full verification remains pending.
