# SWARM-27 — supervised fixture worker loop

Status: partial. Revision: `3889730`.

Input: a two-job backend fixture has a contract job and an endpoint job dependent on that contract. A fixture director supplies the plan once; `swarm.dispatch.next` selects each ready job and launches a local scripted worker in an isolated worktree. Each worker receives only its own run, job, attempt, revision and broker credential through its owner-private launch environment. It submits a finding artifact and result message through Overseer's socket. The fixture director claims its inbox, reviews the artifact and accepts the job.

Expected: the endpoint stays planned until the contract evidence is accepted and its worker exit is confirmed. The director does not need a per-worker user launch. Both jobs retain category/run and revision identity, and worker exit alone does not grant acceptance.

Actual: the first test failed when the worker lacked a broker identity. At `3889730`, both supervised workers submit evidence and results themselves. The first accepted result plus confirmed exit unlocks the endpoint; the second is then dispatched, submitted and accepted. A separate scripted worker receives one targeted advisory and acknowledges delivered then applied. `cargo test --workspace --offline` passed 115 tests (5 unit, 43 existing protocol, 67 Swarm).

Evidence: `daemon/tests/swarm_dispatch.rs` (`supervised_scripted_workers_report_evidence_that_unlocks_dependent_work`, `supervised_scripted_worker_receives_and_applies_targeted_director_advisory`), `daemon/src/daemon.rs`, `daemon/src/swarm/runtime.rs`.

Remaining: the director's plan and review decisions are supplied by the deterministic fixture, not a running inference process. There is no objective-level synthesis/completion gate, live Auto Mode route, or qualified live harness communication path. S0–S5 remain unverified. This criterion stays unchecked.
