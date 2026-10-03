# Frozen historical vectors: baseline

Unchanged production base7d30ec6, test checkpoint075ed9c71f134dc5af29a3c6f99480affa37c602. Standalone daemon and test were source-fresh (`fresh:false`), after package-only clean of overseerd in the coordinator-allocated verify-target. Build20.30s, no-run20.95s. Exact filtered6 tests ran once, serially/nice20,8.72s; terminal exit101. Source/.d/hash/exact-count receipts are in baseline/. Raw Cargo JSON stays private.

- Claude13 and Codex13 reached the intended compatibility assertion and refused the authentic literal historic group: `legacy Overseer MCP argument group is ambiguous`. These are TRUE compatibility REDs, not setup failures.
- Claude14 migration passed.
- Both negative tests passed all unknown/reordered/removed cases, with no replacement process/capability and original saved policy retained.
- Codex14 migration succeeded, then the test wrongly expected bare `read-only` in resume argv. Source `adapters.rs:345–361` and the existing adapter unit at1319 show the intended exact resume form: `-c sandbox_mode="read-only"`. This is a test expectation error, not product migration RED or demonstrated sandbox widening. The next checkpoint changes only that assertion; original baseline output remains intact. Corrected control is UNRUN.

No production correction yet. PR78 remains frozen. Compiler/runtime released immediately after the bounded attempt; no UI/models/owner profiles were used. Root accepted both actual13 REDs before any functional change. Combined15 catalog qualification remains pending.
