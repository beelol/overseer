# Swarm implementation milestone 8 — daemon control states

Pause, Resume and Swarm off now have separate, revision-checked daemon transitions. Pause persists its state, blocks new attempts and queues checkpoint directives for active attempts; Resume returns the run to admission. Swarm off enters `draining`, cancels planned/ready jobs and preserves active attempts so their later result messages can still be recorded. A draining category remains occupied and cannot accidentally start a second active run. Stop remains available after draining.

The fixture admission path checks the run's snapshotted deadline before reserving or launching a new attempt. At expiry it invokes Stop, cancels queued work and reports `run_deadline` without claiming the objective is complete. Replayed successful admission request IDs retain their original identity rather than reserving twice.

Evidence: `daemon/tests/swarm_control.rs` first failed for missing Pause/Resume/Off methods and later for the absent deadline guard; both cases now pass. `cargo test --workspace --offline` passed 4 unit, 25 protocol, 6 admission, 7 broker, 2 control, 3 director, 5 plan, 5 policy, 3 settings and 3 state tests.

Live checkpoint delivery, actual process interruption, periodic deadline evaluation while no request arrives, explicit deadline extension, Swarm off one-agent continuation, and confirmed exit accounting remain unimplemented. The control states are daemon-level evidence only, not a claim that a harness complied.
