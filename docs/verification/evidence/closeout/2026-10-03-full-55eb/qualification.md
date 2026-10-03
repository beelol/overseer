# Frozen 55eb full run

Source: 55ebf1d22760280ba750007c9d49cd315ddda919. Handle67668 terminated exit1. Command: `CARGO_TARGET_DIR=/private/tmp/overseer-closeout-integration-target CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4 nice -n20 scripts/test-all --jobs=1`.

87 of89 stages passed. Rust failed at TUI parity T36 (156 passed,1 failed), preventing most daemon tests from running. PR77 independently corrects the table; this run did not contain it. UI voice failed after264s on manual re-enable after four-crash shutdown: Starting did not reach listening/thinking within30s. Screenshot and original trace retained. Isolated rerun is required before classifying a regression. All subsequent UI stages and final no-leftovers check passed. No full-pass, criterion closure, or merge claim.
