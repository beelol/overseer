# Follow latency: observe the rendered frame

The existing T-30 driver used `pump(5)`, which kept receiving until a minimum wall-clock interval elapsed. In the failed isolated diagnostic (run 37108212222), the first ready file response was handled approximately 146 ms after the disk change was observed. The driver then processed additional refresh replies and waited on another receive; it finally checked readiness at 298.8 ms. The production loop renders before that next receive. This trace establishes a test observation error in that sample, not a production latency improvement.

The correction at d88a65a2 snapshots the queued receive batch before handling it, ticks, renders, then checks the actual filename and added line. The 250 ms threshold, disk-observation starting point, counts, pause, and resume assertions remain. No product code changes. Independent source review accepted the correction.

One corrected run on the diagnostic branch 8d96f2b041a1d14b65ad972bad8a67e94c5fff81 passed: `cargo test -p overseer-tui --test parity t30_follow_in_the_review -- --exact --nocapture --test-threads=1`. Initial edit latencies were 213.20, 130.26, and 186.65 ms; the edit after resuming was 137.70 ms. The run used macOS 26.6.2 (25G83), Rust 1.99.0, two build jobs, and fixture harnesses. Its diagnostic instrumentation is deliberately excluded from the implementation branch.

Evidence: before.log, after.log, runner.txt. CI run: https://github.com/beelol/overseer/actions/runs/37109056971 . One isolated fixture test passed; this is not a full-suite or broad performance guarantee. The combined clean-clone full gate is still pending.
