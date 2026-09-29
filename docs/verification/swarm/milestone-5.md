# Swarm implementation milestone 5 — bounded director inbox

The fixture-only director batch transition selects at most 20 queued messages and at most 32 KiB of inline payload. A small batch waits until its oldest message is five seconds old; a full count or byte batch becomes ready immediately. Claiming a batch records a single active director turn, its message membership and delivered phase in one transaction. A second claim stays busy through daemon restart. Completing the authenticated fixture turn marks those messages applied once, and Stop prevents any new director claim.

The same fixture-only guard now covers attempt registration and exit confirmation. With that guard off, the daemon cannot fabricate a running worker or a confirmed exit through those temporary public RPCs. This guard is a safe default during development; it is not a substitute for a real runtime that owns launch and liveness evidence.

Evidence: `daemon/tests/swarm_director.rs` first failed because the batch RPC was absent, then passed count/byte/age/restart/Stop cases. `daemon/tests/swarm_broker.rs` proves fixture calls are rejected without opt-in. `cargo test --workspace --offline` passed 4 unit, 25 protocol, 6 broker, 3 director, 5 plan, 4 policy and 3 state tests.

No model turn is invoked by this transition. A live harness delivery path, director process ownership/replacement, message-level permission checks, failed-turn recovery, targeted peer routing and semantic review remain unimplemented. The word “applied” here records the fixture completion call only, not that a real director model processed the messages.
