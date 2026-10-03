# Initial host baseline and same-package diagnostic

Root executed these checks; the implementation agent did source-only authoring.
Production Voice host and daemon behavior remains frozen at fe643.

| Stage | Source / artifact | Actual result | Scope |
| --- | --- | --- | --- |
| Original combined full | fe643 | Voice UI failed at original line 434 | Retained full failure, not dismissed |
| Original isolated Voice | fe643, same package | Re-enable timeout at line 434 again | Failure excerpt retained here; no post-enable daemon state capture |
| Initial host baseline | test source 0ad0f86, unchanged fe643 Voice class | 1 RED, 1 PASS | Late Starting replaces live Listening; explicit Off/re-enable works |
| Diagnostic packaged Voice | diagnostic source 0ad0f86 (setup docs d2759ab), original VSIX SHA-256 `cc250107fe2b875b8d9aced1d8452f9c50e5906864a7784f63d420262e98ffbd` | Terminal exit 0, all 32 checks pass | Same-package diagnostic rerun; timeout catch was not reached |
| Expanded host fixture checkpoint | 11 test cases, unchanged production class | **UNRUN** | Promise-controlled concurrent, null-cache, disconnect, title and badge ordering |

Root's host log is preserved exactly in `host-baseline.log`: the intended
assertion observed `starting` instead of `listening`, and the Off/re-enable
control passed. It establishes an independent host ordering defect. It does
not prove that defect caused either original packaged failure.

`package-result.json` and `package-scenario.log` preserve the current 32-check
pass. Only two purposeful screenshots are included: `stopped.png` shows the
four-crash Off reason, and `reenabled.png` shows the later Listening stage. The
other generated screenshot changes are not staged. No timeout diagnostic JSON
exists because its catch did not execute. Root reported the owned scenario
`/private/tmp/ovs-ui-1YovDu` has zero remaining processes except its checker.

Original failure sources remain at
`/private/tmp/overseer-full-fe643-before-voice-rerun` and
`/private/tmp/overseer-voice-fe643-isolated.log`; isolated failure excerpt is
preserved in `original-isolated-failure.txt`. Root reported cleanup of original
`/private/tmp/ovs-ui-I0a9sY` too. The prior failures remain unexplained
intermittent failures; there is no daemon/listener-versus-webview causal finding.

Expanded fixtures retain the original host baseline and add no production fix.
Each host receives a fresh real Voice module bound to its own mock VS Code
boundary. Deferred gets/rosters capture independent values; explicit promise
releases determine ordering, with setImmediate used only to drain completion
callbacks. Root must execute and classify each reached assertion before the
planned host correction. No new test bodies or runtime were run by the author.
