# SWARM-44 — one owner per hypothesis and path, in one daemon claim ledger

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`). Reproduce with `cargo test --offline -p overseerd --test swarm_claim_ledger -- --test-threads=1` and `cargo test --offline -p overseerd --test swarm_plan -- --test-threads=1`; the joined S1 replay is opt-in (`swarm_atlas.rs`, needs the disposable PostgreSQL runner in `fixtures/swarm/atlas-v1`).

## Design: one ledger

Gate S landed first with `areas` (an ordinary agent's paths, from its `claim` tool, the owner's `agent.area` or a resolved conflict's `assign`); Swarm keeps its jobs' claims in `swarm_claims`, which carry what an area has no room for (job, read or write, plan revision). `daemon/src/claims.rs` makes them one ledger: every writer of either table asks it inside the same store lock and transaction before it writes, so a Swarm job and an ordinary agent can never both own one path.

- Across the two kinds only exclusive ownership conflicts: an agent's area counts as a write claim; a Swarm `write` claim is exclusive; a Swarm `read` claim (independent read-only analysis of a pinned revision) never conflicts with an area. Overlap is the same path or a directory that holds it.
- Between Swarm jobs the Swarm rules still apply; between ordinary agents Gate S's still apply (overlapping areas are allowed and become conflicts it shows). Gate S's tests are unchanged and pass.
- Scope: a swarm's claims meet the areas of agents in the repositories it is scoped to (a fixture run with no scope meets every repository). Only live agents hold areas.
- A claim is refused, never queued or reassigned. The refusal is recorded once in `claim_refusals`; the director hears it in its durable inbox (sender `ledger`, kind `claim`, no attempt, assigns nothing), and the ordinary agent's side hears it as a `claim_refused` event on the agent's run and a card in Overseer's conversation. A job whose planned write claim falls inside an agent's area waits at admission (`resource_conflict`, naming the holder).
- `claims.ledger` reads the whole ledger: Swarm jobs' active claims, live agents' areas and the refusals.

## Clauses and tests

| Clause | Test |
| --- | --- |
| J2/J4 claim the same investigation: one owner | `one_hypothesis_owner_with_explicit_reproducer` (`swarm_plan.rs`): a second exclusive claim of `hypothesis:tenant-leak` is refused; joined S1 (`atlas_s1_backend_evidence_flows_through_swarm_review`, opt-in): J4's overlap report reaches the director in the next batch and J2 keeps the trace |
| …the director redirects the other | joined S1: the director redirects J4 to the signed-URL boundary and J4 acknowledges delivered and applied |
| J1 reads the same file for a different question: read-only allowed | `shared_reads_and_exclusive_claims_span_categories` (`swarm_plan.rs`: read claims of one file coexist across categories); `an_ordinary_agent_and_a_swarm_job_never_both_own_one_path` (a read claim inside an agent's area is allowed) |
| Exclusive claims held atomically across Swarm runs and categories | `shared_reads_and_exclusive_claims_span_categories`; the new test (a second category's swarm is refused the path J2 holds) |
| …and ordinary agents, in one daemon ledger | `an_ordinary_agent_and_a_swarm_job_never_both_own_one_path` (new): J2 first, the agent's claim, the owner's area and a directory holding the path are refused and name J2 and its director; the agent first, J2's claim is refused and names the agent and J3's admission waits; the ledger shows one owner per path; both sides see each refusal once; nothing is reassigned; another repository is another scope; after the director narrows J2 the path is free. `racing_claims_from_an_agent_and_a_swarm_job_admit_one_owner` (new): twelve paths claimed at once from both sides, exactly one owner each |

Red first: before the ledger, the agent's claim of a path J2 held succeeded (`{"is_error":false,"text":"Claimed routes."}`) and both racers won `src/module0.ts`.

Runs on 2026-09-28, one file at a time with `--test-threads=1`: `swarm_claim_ledger` 2 passed; `swarm_plan` 13; `swarm_admission` 41; `swarm_gate_s` 3; `overseer` (Gate S) 24 (one timing test, `ac189_overseer_keeps_agents_on_task`, failed once under the machine's load and passed alone; `ac185` needed the new `claims.ledger` method classified as a read); unit `claims` 4.

Boundary: scripted director decisions and a generic ordinary agent; no live harness. The joined S1 replay was last run with PostgreSQL at `3733cea2` (all 17 Atlas tests); it was not rerun for this change, which adds no rows for its resources (`atlas-db-*`) and no ordinary agents.
