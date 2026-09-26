# SWARM-07 — one application agent ceiling

Status: partial. Revision: `5b39f0f` (fixture-only daemon).

Input: `agents.max_active` defaults to 9 and is saved once for the application. Tests set it to 1, 2, 3, 4 or 33. Manual `/bin/sleep` runs, synthetic native children, registered Swarm attempts and running directors share one daemon and SQLite store. The Swarm run policy can lower `max_workers` but cannot set the former `max_executing` total ceiling.

Expected: a manual start reserves a top-level slot before workspace setup, a registered worker and its linked run count once, and a native child remains within its parent slot. Swarm waits at the limit; manual start receives `agent_limit` with active agents. A stopping director retains its slot until confirmed worker exit; lowering the app maximum drains without interrupting existing work. Saved older `max_executing` settings must not stop new Swarm creation or override the app limit.

Actual: focused tests first found missing app-limit RPCs, duplicate per-Swarm policy acceptance, and failure to load older saved policies after that setting was removed. At this revision the tests cover persistence across daemon restart, invalid zero, concurrent manual starts competing for one slot, manual/Swarm admission in both directions, native-child exclusion, 32 workers under an explicit 33-slot app setting, stopped-director release and a three-to-two limit drain. The obsolete policy key is rejected on new writes and ignored on reads of older saved settings, retaining other policy fields. Admission uses the app cap; a separate run `max_workers` still bounds Swarm workers.

Evidence: `daemon/tests/swarm_admission.rs`, `daemon/tests/swarm_settings.rs`, `daemon/tests/swarm_runtime.rs`, `daemon/src/store.rs`, `daemon/src/daemon.rs`, `daemon/src/swarm/admission.rs`, `daemon/src/swarm/settings.rs`. Run `cargo test --offline -p overseerd --test swarm_admission --test swarm_settings --test swarm_runtime -q` for the focused fixtures. `cargo test --workspace --offline -q -- --test-threads=1` passed 198 tests with 11 ignored at `5b39f0f`; no paid model was used.

Remaining: a real director process does not yet consume and release its slot through normal launch/recovery, separately launched reviewer capacity is not qualified, and `max_active=1` serial director execution is not demonstrated. Benefit planning can still overstate the parallel pool before app admission clamps it. Keep SWARM-07 unchecked.
