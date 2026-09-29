# SWARM-45 — independent reproduction and duplicate findings

Status: verified at fixture scope on 2026-09-28 (`claude/auto-swarm`). Reproduce with `cargo test --offline -p overseerd --test swarm_reproduction -- --test-threads=1`.

## Built

`daemon/src/swarm/findings.rs`, four fixture-API director methods (the same gate as `swarm.conflict.open`) and a readout:

- `swarm.finding.record`: the director's record of one defect (title, the discovery it rests on, evidence entries of one job's submitted artifact about one endpoint). Recording again only adds evidence; an artifact that is not a submitted result of that job, or was changed after submission, is refused.
- `swarm.finding.merge`: a duplicate finding's evidence moves into the one kept, every endpoint's entries intact; the duplicate stays as `merged` for provenance and takes no more evidence. Two findings that each have a reproducer cannot be merged until one is kept.
- `swarm.reproduce`: an explicit independent reproducer for one recorded finding, created as a plan revision, with its own budget in one unit. It may not depend (directly or not) on the jobs whose evidence it reproduces, nor write a resource they claim. One finding has at most one reproducer, and a discovery message is the source of at most one: repeats, duplicate discoveries from other workers and other findings citing the same discovery return the existing reproducer and create no job.
- Admission: a reproducer is an ordinary worker in the queue (the worker limit, wave and run allocation apply) and is also bounded by its own budget: each admitted attempt's estimate is charged to it, and an estimate that would exceed it is `blocked` / `reproduction_budget`.
- `swarm.findings`: each finding by endpoint with its evidence, how many jobs agree, and its status. Agreement is shown, never counted: an endpoint is confirmed only by accepted reproduction evidence (an artifact of kind `reproduction` in an accepted review), and once a reproducer exists only its accepted reproduction confirms. A finding is `candidate`, `partially_confirmed`, `confirmed` or `merged`.

Before this, a reproducer was only a job the director added through `swarm.revise` (as in the joined S1 replay's J7): nothing tied it to the finding, gave it a budget of its own, or stopped a second discovery from adding another.

## Clauses and tests

| Clause | Test |
| --- | --- |
| J7 is explicitly created as independent reproduction | `a_reproducer_is_an_explicit_budgeted_job_and_duplicate_discoveries_make_one`: `swarm.reproduce` for the recorded finding creates J7 as revision 2, ready; the finding names J7 as its reproducer; a reproducer depending on J2 or writing J2's database is refused as not independent; an unrecorded finding is refused |
| …with its own bounded job and budget | same test: a zero budget is refused; J7 waits for a worker slot like any job (`worker_limit` while J2 and J4 run); an estimate over its 200-point budget is `reproduction_budget`; after a rejected first attempt charged 150, a 100-point second attempt is refused and a 50-point one admitted; the readout shows 200 spent |
| Duplicate discovery messages cannot create repeated reproducers | same test: replaying the source D1, J4's duplicate discovery and another finding citing D1 each return J7 with `duplicate: true`; no J8, J9 or J10 exists and the revision stays 2 |
| Merge duplicate findings without losing endpoint-specific evidence | `merged_findings_keep_endpoint_evidence_and_agreement_is_not_proof`: J5's duplicate of J2's PATCH finding is merged in; PATCH keeps both J2's and J5's evidence and DELETE keeps J2's; the duplicate is `merged` and refuses more evidence; recording again drops nothing; evidence from the wrong job is refused; J4's attachment finding is linked to D1 but stays its own finding |
| …or counting agreement as proof | same test: two agreeing workers leave the finding `candidate`; the attachment finding is confirmed only after J4's reproduction is accepted; after J7's independent reproduction is accepted the PATCH endpoint is confirmed and DELETE is not (`partially_confirmed`), with J2, J5 and J7's evidence all kept |

Red first: both tests failed with `unknown method swarm.finding.record` before the build.

Runs on 2026-09-28, one file at a time with `--test-threads=1`: `swarm_reproduction` 2 passed; `swarm_admission` 41; `swarm_conflict` 11; unit `swarm::` 14; `overseer` `ac185` (every method classified) 1.

Boundary: scripted director decisions on a fixture swarm shaped like S1 (no Atlas backend in these tests; the joined S1 replay still adds J7 through `swarm.revise`). The native director's MCP tools do not yet include these methods.
