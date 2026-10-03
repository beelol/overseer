# Core native answer fixture correction

Baseline source `9040eb52bc58baaf24685e1b08744e955c145982`: the coordinator's exact eight-case batch completed with **7 passed, 1 failed**. The attached case-05 raw output records the failure at `daemon/tests/pending_requests.rs:643`: the shared `reject` helper requires an empty native response list.

This test had intentionally written one exact frozen decline before invoking that helper. Its first typed write, exactly-one denied-operation row, and cross-agent no-reroute refusal passed before the helper failed. The compatibility-loop iteration and later assertions were not reached. This is a fixture assertion mismatch, not an authority/product RED.

The test-only correction calls the actual repeat-answer RPC directly, asserts `already_answered`, and asserts that the exact one previously written frozen native response remains unchanged. The shared helper, other negative cases and all production code remain unchanged. Removing the duplicate-answer refusal or emitting any additional/different native response will fail the corrected assertions. Existing once-only ledger and no-effect assertions remain.

The correction is source-authored and **UNRUN**. No Cargo, daemon, shim, UI, native provider or paid call was started by this editor. The coordinator owns exact rerun qualification. Baseline receipts preserve source/artifact proof and the seven successful exact names; they do not qualify the corrected source or broad AC274. Owned cleanup count was zero; unrelated-process assessment remains with the coordinator.
