# Expanded internal runner baseline: missing-boundary evidence

Authored fixture checkpoint975e593b2848b4a5aa7bf2a5fb9760c0b5ea70b4, unchanged production runner stub23f62e8 (`runner_unavailable`). No confirmed program install, native hook, model, owner profile or production instance. Coordinator-owned serialized slot: nice20/jobs1/threads1, package-only overseerd clean, source-fresh no-run20.63s. Exact10 named cases ran once0.88s, terminal101,0passed/10failed. JSON artifact receipt, source/.d/binary SHA256 and exact nonzero count retained; full CargoJSON remains private.

All ten failures reached their independently specified missing runner boundary, with no startup/watchdog/setup failure:

- UTF8/multiline expects exact Output; stub returns unavailable.
- Silent hang expects timeout; stub returns unavailable.
- Combined stdout/stderr flood expects output_limit; stub returns unavailable.
- Changed program expects program_changed; stub returns unavailable.
- File/network/process probe unsandboxed controls all passed (including same0555 chmod→write, other-run write, owned TCP/Unix listeners, fork and independent posix_spawn). The sandbox output assertion then received unavailable. This proves fixture accessibility, not runtime sandbox denial.
- Environment witness expects Output; stub does not execute it.
- Empty-output/nonzero/invalid-text case stops at the first empty Output assertion.
- Blocked-stdin/separate-flood case stops at blocked stdin's timeout assertion.
- Cancellation case stops at prelaunch cancelled assertion.
- Symlink case stops at the first leaf unsafe_path assertion.

Later assertions are authored but NOT exercised: empty case's nonzero/invalid-text modes, blocked-stdin case's separate floods, running cancellation/startup/reap, and remaining scratch/ancestor aliases. Even first assertions are missing-boundary RED, not evidence that hostile operations are isolated. No AC271–273 qualification or completion claim.

Each case uses a45s independently bounded exact-test worker. Compiler/direct-control children have15s/5s guards; groups kill/reap on unwind, concurrent diagnostics retain at most64KiB, private roots and Unix listener paths are owned by parent cleanup. Synthetic witnesses use actual PID startup for future running/cancellation/reap checks. `cleanup.json` confirms zero workers/helpers or worker roots remain after this baseline. Production runner code is unchanged and remains internally unavailable.

The raw test log retains its original EOFblank warning; source/docs whitespace checks passed. Root owns the next implementation/gate decision. No further runtime attempt was made.
