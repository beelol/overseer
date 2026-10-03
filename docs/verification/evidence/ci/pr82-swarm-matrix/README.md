# Focused Swarm matrix verification

Test-only source: `56351ec39a35413526f8cb835a9d44689b1dd712`, based on `abf572ef53705db0e8f8b32e72a46057661057fc`. Root independently accepted the bounded marker/release setup before execution. Admission, account booking, production fixtures and every original cell assertion are unchanged.

The preserved PR82 CI receipt records two actual failed tests and a separate 40-minute job timeout. The manual/off matrix failure was `slots: 0` versus `1`. The original manual `hold` prompt completed immediately in the existing fixture; the corrected test uses its existing bounded `hold parent` release gate and keeps the parent alive through the snapshot.

On 2026-10-03, one exclusive runtime slot used `nice -n 20`, one Cargo job and one test thread. Only `overseerd` package artifacts were cleaned in the retired verify target; dependencies were retained. A fresh standalone build and test no-run preceded source/manifest/.d/hash checks (`artifact-proof.json`, `fresh: false`). Actual counts were:

- `cargo test -p overseerd --test swarm_native four_way_launch_matrix_auto_manual_by_swarm_on_off -- --exact --nocapture --test-threads=1`: 1/1 passed, 21.09 seconds.
- `cargo test -p overseerd --test swarm_native -- --nocapture --test-threads=1`: 19/19 passed, 139.28 seconds, including four existing helper cases.

Source stayed at the reviewed checkpoint through verification. Scoped process cleanup found zero owned leftovers and zero other leftovers; no processes were killed.

## Separate unchanged T30 diagnostic

The existing parity binary from frozen `abf572ef53705db0e8f8b32e72a46057661057fc` ran only `t30_follow_in_the_review`, alone at verified niceness 20: 1/1 passed in 8.57 seconds. Latencies were 107.886708, 134.267416 and 148.598417 ms against the unchanged 250 ms limit. Its original support helper performed an actual daemon rebuild from that clone; the daemon hash and mtime changed, while parity and TUI hashes stayed unchanged. The after-build daemon .d names the same frozen-source clone. No helper was stubbed and no benchmark source or threshold changed. Scoped cleanup again found zero leftovers.

The CI timing failure was not reproduced alone. This is a separate diagnostic, not evidence that the Swarm fixture correction fixes T30. The full suite on this corrected branch is pending; no criterion is newly verified and the draft is not ready for merge. These focused checks used synthetic fixture inputs, no paid calls, owner profiles or private assets.
