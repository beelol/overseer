# Milestone 17 — preserve control state through director recovery

The SWARM-30 fixture exposed two unsafe control transitions. First, Pause followed by Resume could clear an unresolved director stall. Second, confirmed replacement after a paused run entered the stall restored `planning`, which would allow new admission without a user Resume. Both were covered with failing tests before fixes.

The daemon now rejects Pause and Resume while director termination is uncertain and stores the run's prior status durably. Repeated unknown reports and a daemon restart keep that status; confirmed death advances the director generation and restores `paused` for a formerly paused run. Older databases gain the new column through the schema migration without losing existing runs.

Verification: `cargo test --workspace --offline` passed all 76 tests. SWARM-30 remains partial because no real director process liveness proof, replacement routing, or live model execution exists.
