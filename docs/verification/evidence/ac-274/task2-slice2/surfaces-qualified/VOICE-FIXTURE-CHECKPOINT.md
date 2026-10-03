# Self-contained simulated Voice fixture — source only

Prior evidence is frozen at40a40bc: five reference-phone cases passed and the original Voice case failed before readback assertions because no listener existed beside the daemon. Those streams are preserved and are not authority/product RED.

Only the one pending Voice fixture and its local helper change. The helper follows the existing build-once Voice test pattern, but requires actual successful Cargo completion (never the old `ok || bin.exists()` fallback). It builds the listener from this repository, honors the allocated target (including relative path resolution), sets one Cargo job and requests nice20. A failed build is a fixture setup failure. The test pins that path and enables fake synthesis plus a disposable synthetic cue log alongside simulated/scripted input. Its temporary voice folder outlives the daemon; original typed permission/readback/revision/provenance/native-response assertions remain unchanged.

Source-authored only: diff whitespace review, no Cargo/rustc/listener/daemon/test/native synthesis/UI/provider/private audio. Runtime qualification must prove same-source listener executable/`.d`/hash and exact one selected case, then report any reached behavioral failure separately. This fixes the suite-order dependency; external env-only qualification would not fix the standard fixture.
