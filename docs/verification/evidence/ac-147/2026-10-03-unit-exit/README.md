# Line-diff unit shutdown qualification

Base: `2a1bd2a12a5fe35dfb7d91e1fa27053ff6d203c2`. Runtime: Node 24.20.0 on macOS arm64. Only the final test exit changes: set `process.exitCode`, then let Node drain normally. Assertions, failure counting, and the resulting success/failure code are unchanged.

The full run passed 1601 Rust tests, then its line-diff unit child stayed alive with essentially no CPU use for more than four minutes. A one-second process sample showed the main thread inside `process.exit` → `NodePlatform::Shutdown` → `WorkerThreadsTaskRunner::Shutdown` → `uv_thread_join`, while the V8 worker was in baseline compilation → `HeapAllocator::AllocateRawWithRetryOrFailSlowPath` → `CollectionBarrier::AwaitCollectionBackground` → condition-variable wait. The stack establishes a runtime shutdown wait; it does not establish that every possible Node shutdown hang has been fixed.

The coordinator sent SIGTERM only to that confirmed child, preserving the full runner. Its unit stage is FAILED, 27/28; a later isolated success does not turn that full run green. The full source remained unchanged.

A bounded original-source probe reproduced the hang on attempt 4, after stdout reported all eight assertions passed. The natural-exit variant completed 30/30. `repeat-probe.json` contains both results. The actual changed checkout then completed another 30/30 runs with eight assertions each (`changed-repeat.json`) and all 28 unit files (`changed-unit.log`). Each probe had a finite subprocess timeout; timed-out probe children were killed and reaped by the Python runner. No paid calls, compiler build, UI, or production processes were involved.

Combined full qualification of the corrected source remains pending. The earlier full run must retain its failed unit stage.
