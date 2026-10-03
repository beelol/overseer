# PR82 Swarm matrix fixture correction

Base: `abf572ef53705db0e8f8b32e72a46057661057fc`. This is test setup only; admission, booking and production code stay unchanged.

CI run `37135720951` checked merge `faebefcd3db1455364cb5bc1ece69821b4027501` (parents `36070dfca4074ac4bc15ce53ff71bc9298cceba8` and PR82 `5224411300c3096236eaf9f8172012667e9c2777`). The test, Codex fixture and workflow blobs match the base exactly. The actual failure was manual/off `slots: 0`, expected `slots: 1`; the other three cells matched their expected launch paths, bookings, slots and Swarm counts.

The manual prompt was `hold`, but the real fixture only delays `browser check`, `hold parent` and continuation prompts. It completes `hold` immediately, even with the requested eight-second delay. `wait_status(running)` and the later slot RPC can therefore observe different lifecycle moments.

Reuse the existing bounded `FIXTURE_HOLD_PARENT_GATE` with the supported `hold parent` prompt for both ordinary and Auto roots. Each cell resets a private trace and release file, waits for the actual fixture's turn-model marker, verifies running and an unexpired/unreleased barrier, then records the original strict cell snapshot. Release explicitly after the snapshot and keep the existing run cleanup. A per-cell guard releases on unwinding too. No longer sleeps, changed admission expectations or source-mirror assertions.

The CI failure is actual baseline evidence; corrected exact matrix and full `swarm_native` verification remain unrun until allocated. The fixture gate's native deadline is 30 seconds; reviewed snapshots must fall inside a shorter 25-second setup budget and still report running, so barrier timeout cannot count as a held snapshot.

TUI T-30 is separate: CI recorded 325.687708 ms against the named 250 ms rendered-Follow benchmark. Its actual disk-edit/render/hunk checks remain intact. Rerun alone before classifying a regression; do not raise the threshold. The job also exceeded its independent 40-minute budget and never completed Rust or reached extension checks. Neither cancellation nor a future budget increase erases either recorded test failure.
