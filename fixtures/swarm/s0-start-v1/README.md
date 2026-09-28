# S0 normal start v1

`cargo test --offline -p overseerd --test swarm_start` exercises Swarm scenario
S0 through the daemon's normal start, `swarm.start`: a category, an objective
and a repository give one read-back (approved accounts and their readings, the
allocation rule, the worker ceiling, the deadline and the director), and the
owner's yes to that read-back's digest commits the run and launches the
director through the one launch path.

`director.py` is the qualified director only under the fixture API
(`OVERSEER_SWARM_FIXTURE_API=1` and `OVERSEER_SWARM_FIXTURE_DIRECTOR`). It plans
three audit jobs, offers `members` to the approved Claude account (an audit run
refuses a native worker before any booking), dispatches three supervised
`worker.py` processes through Swarm admission on the fixture target, waits for
the test's gate file, routes discovery D1 to `members`, accepts each
evidence-backed result and completes the run. It writes each step to a trace
file. The restart test kills the daemon while every process waits on the gate.

Everything runs on the local generic harness in temporary workspaces. The
audit evidence is fixture text; the director's choices are scripted. This
demonstrates the normal start, the durable protocol and supervised recovery,
not model reasoning, a live account allowance or a qualified harness transport.
