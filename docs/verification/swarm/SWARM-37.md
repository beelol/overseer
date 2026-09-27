# SWARM-37 — large-run status and controls

Status: partial. Code revision: `e773082`.

Input: a local daemon fixture plans 100 independent jobs, then sets 32 job records to `running`, 8 to `submitted`, 4 to `blocked`, and leaves 56 `ready`. It creates a director owner without a linked process, pages filtered jobs, and restarts the daemon. The fixture deliberately does not launch 32 workers.

Expected: the read API gives aggregate job counts and stable filtered pages without loading every transcript. It does not mistake a running job record or registered attempt for a live supervised worker. Director identity remains visible without leaking its private token. The readout survives restart.

Actual: `swarm.get` returns `job_counts.total=100` with the four state counts, zero `active_worker_processes`, zero `registered_attempts`, and the director owner status with no process ID. `swarm.jobs` returns 20 and then 12 running jobs with a cursor, 4 blocked jobs, and rejects an unknown status filter. After restart the counts and submitted filter are unchanged. The focused test and affected Swarm suites passed: `cargo test --test swarm_state --test swarm_director_owner --test swarm_control --test swarm_admission --offline -q` (51 tests); the focused state suite passed again after the final process-status query adjustment (15 tests). The full offline Rust suite also passed with `cargo test --workspace --offline -q`; its unrelated regenerated TUI evidence was restored to the clean pre-test state. `git diff --check` passed.

Remaining: this is daemon readout only. The required VS Code fixture has no director/aggregate/blocker/account view or worker detail drilldown. No 32 supervised workers were launched, and Pause/Stop latency has not been measured in the required UI fixture. Account usage and limiting constraints still depend on the shared Auto Mode admission contract. SWARM-37 stays unchecked.

At `52eea97`, a separate one-attempt Atlas fault replay measured Stop acknowledgement at 7 ms during 2,000 duplicate progress updates. It does not exercise the required 100-job/32-active-worker UI fixture or Pause latency, so SWARM-37 remains partial.
