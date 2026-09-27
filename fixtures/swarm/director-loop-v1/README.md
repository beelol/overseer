# Scripted director/worker loop v1

`cargo test --offline -p overseerd --test swarm_director_loop` launches one
supervised local director and two supervised local workers. The director owns
the category plan and launch decisions, commits a synthetic benefit estimate,
admits both workers against a synthetic quota snapshot, claims bounded inbox
batches, sends a focused D1 advisory to J4, accepts two artifact-backed results,
and records the final checked completion. J2 cannot submit its result until J4
applies D1. No test code sends a per-worker launch or director decision.

The test asserts exactly two worker launches, two review decisions and one
completion. It also verifies that D1 precedes J2's result and that J4 applied
the targeted advisory. Everything runs inside temporary workspaces under the
local generic harness. This demonstrates the durable protocol and supervised
coordination loop, not model reasoning, actual backend correctness, live account
allowance, qualified provider communication, or credential-safe delivery to a
different user/account. The scripts select a known fixture target and evidence
shape; their assertions do not qualify autonomous semantic decisions.
