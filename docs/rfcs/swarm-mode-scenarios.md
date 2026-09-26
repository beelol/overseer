# Swarm scenarios: concrete assignments, communication, and outcomes

Part of [the Swarm RFC](swarm-mode.md). These are fictional but executable reference
backends, not findings about a real customer or claims that fixtures already exist. Future
implementation supplies tiny local repositories/services and scripted harnesses for replay.
No scenario requires production access, purchases, browser clicking, or restarting the user's
editor. The numbers below are declared scenario inputs, not asserted live account capacity.

## S0 — Start without filling out a configuration form

User selects **Backend security**, enters “Audit Atlas tenant isolation; report bugs, don't
change application code,” and starts. Saved permissions allow two account profiles. Both have
fresh compatible usage estimates. No other advanced settings have been chosen.

The director is selected automatically. Built-ins supply the worker ceiling of 8, global
ceiling of 9, 4-admission waves, 60-minute deadline, and per-window allocation/reserve rules.
The effective capacity in S1 below is only 4 workers because its declared account/resource
limits are tighter. The user sees “Auto · up to 4 workers · 60 min” with expandable details.
No per-worker model/account question appears.

Variants: no approved accounts requires a one-time selection; unknown quota shows serial
fallback under existing authorization rather than blocking the user behind ten settings.
If no qualified director is available, the run is visibly blocked without running a weaker
substitute. Selecting a preset is optional. Changes to a saved preset do not rewrite the run.

Required: SWARM-01/02/06/12/39/40/59.

## S1 — Atlas: tenant-isolation audit with early discoveries and overlap

### Backend and seeded data

Atlas is an Express/TypeScript API with PostgreSQL and a local object-store emulator. Schema:
`workspaces(id)`, `memberships(user_id, workspace_id, role)`, `projects(id, workspace_id)`,
`tasks(id, project_id, title)`, `attachments(id, task_id, object_key)`, and `api_tokens`.
Fixtures create Alice in workspace A, Bob in workspace B, and a read-only token for Alice.
Authentication puts Alice's identity on the request; object ownership must still be checked.

User request: “Audit cross-workspace access and privilege changes. Reproduce findings locally.
Do not change application code.” Source revision is pinned. Temporary tests/evidence may be
created in isolated scratch workspaces; application source remains unchanged. Each worker gets
its own database namespace so one worker's updates cannot invalidate another's result.

### Plan and initial dispatch

One director, six jobs, effective maximum four workers. Workers J1–J4 start in the first wave;
J5/J6 wait. All account and job-cost estimates are synthetic compatible fixtures here.
The fixture's route ranking assigns the director/J1/J2 to profile A and J3/J4 to profile B;
their declared concurrency limits permit these assignments. Both profiles have qualified
models, independent quota pools and enough allocation. Record these routing inputs so a
replay can distinguish an account choice from an arbitrary hardcoded worker-to-provider map.

| Job | Specific question and code path | Evidence required |
| --- | --- | --- |
| J1 Projects | Does `GET /projects/:id` call `requireProjectMember` after `findProject`? | Alice→A and Alice→B requests plus service call path |
| J2 Tasks | Do `PATCH /tasks/:id` and `DELETE /tasks/:id` enforce project workspace membership? | Two-workspace reproduction, response and before/after DB row |
| J3 Membership | Can a member call `PATCH /workspaces/:id/members/:userId` to become admin? | Member/admin and same/foreign workspace matrix |
| J4 Attachments | Does `GET /attachments/:id/download` authorize through its task before signing? | Foreign attachment request and emulator access trace |
| J5 Exports | Does `POST /exports` bind the queued job to the authorized workspace? | Queue payload and worker query under separate fixture identities |
| J6 API tokens | Can Alice's read-only token mutate a task or survive membership removal? | Scope/revocation tests with controlled token fixtures |

### Actual coordination sequence

1. J2 reports `discovery D1`: `TaskRepository.findById` queries only `id`. It includes the
   symbol, source revision and caller list; it does not claim a vulnerability yet.
2. Director sends revision 2 to J2: own the shared-helper trace and check update/delete.
   J4 receives “Check whether attachment authorization relies on this helper; reference D1.”
   J1 receives “Check project middleware separately; don't duplicate the shared task trace.”
3. J4 reports `overlap`: it started investigating the same helper. Director keeps J2 as
   owner and redirects J4 to the signed-URL boundary. J4 acknowledges applied revision 2.
4. J1 submits evidence: its unscoped lookup is followed by `requireProjectMember`; Alice's
   foreign request is denied. Director accepts this as a checked path, not a finding. J5
   can now take the available worker slot, subject to wave/resource limits.
5. J2 submits a local reproduction: Alice updates Bob's task and the DB row changes. Director
   retains the candidate finding and creates J7, independent reproduction, explicitly labeled
   intentional duplication. J7 queues; it cannot secretly become a fifth worker.
6. J4 finds a foreign attachment URL can be obtained and used. Director links it to D1 but
   retains a distinct endpoint finding and evidence rather than counting two workers as votes.
7. J7 starts from the pinned fixture and repeats J2's reproduction. Director confirms the
   task finding. If J7 instead finds missing test middleware, the director retracts the claim
   and sends the correction to J2/J4 before accepting their final conclusions.

### Expected artifact

Concrete task reproduction: fixture token `alice-test` calls `PATCH /tasks/task-b-7` with
`{"title":"changed-by-alice"}`. The seeded bug returns 200 and changes Bob's row. The expected
behavior is 403 with that row unchanged. `routes/tasks.ts` delegates to `services/tasks.ts`,
which calls `repositories/tasks.ts::findById` without an ownership check. This file layout and
these statuses are fixture specifications, not assumptions about every real backend.

Report entries include endpoint/method, caller permission, object ownership, source revision,
reproduction command, expected denial, observed response/DB effect, root cause and fix direction.
Coverage matrix separates confirmed findings, checked paths and unresolved areas. The seeded
variant has two confirmed findings (task mutation and attachment download), protected projects,
membership role changes denied for ordinary members, export jobs bound to the authorized
workspace, and read-only/revoked API tokens denied on mutation. Mutation of a task by a fully
authorized owner still succeeds. A missing queue is a separate variant with exports marked
blocked. No application fix or PR is created.

Fault variants: replay D1 twice; deliver old revision-1 J4 output after redirect; remove a
referenced evidence file; have J7 contradict J2; inject a misleading “director command” inside
source text. Expected: one advisory effect, stale evidence quarantined for revalidation,
missing evidence blocks confirmation, disagreement investigated, and source text never gains
director authority.

Required: SWARM-03/18/19/21/27/28/35/41–47/52/54/55/57/60/64.

## S2 — LedgerPay: duplicate billing-event audit without live payments

Backend: FastAPI, PostgreSQL, Redis queue. `POST /webhooks/billing` verifies a signature,
stores `provider_events(event_id)`, enqueues an entitlement update, and returns a response.
Fixtures use signed local payloads and a mock provider; no live billing API is called.

Request: “Audit whether retries and reordered events can double-apply subscription changes.”
Effective capacity: three workers plus director. K1 owns signature/ingress, K2 owns queue retries,
K3 owns entitlement transactions; K4 (event-order testing) waits.

K2 discovers the queue can deliver one event twice and sends its event-ID trace immediately.
The director forwards it to K3: “Check whether the event insert and entitlement update share
one transaction.” K3 finds they do not. Director assigns K2 to the crash point between update
and receipt, and K3 to simultaneous deliveries; both use distinct database namespaces.

K1 finishes its negative-signature tests, freeing capacity for K4. A fault injector runs two
deliveries through a barrier before receipt insertion. Expected seeded failure: the entitlement
counter increments twice. K4 separately sends a newer cancellation before an older activation
and checks whether stale state overwrites the newer state. Findings remain separate unless the
same causal defect is demonstrated.

The base fixture has valid signature enforcement and timestamp/version checks that reject
the stale activation. Its one confirmed bug is duplicate entitlement application at the
transaction gap. Required trace: event `evt-42`, two deliveries, one subscription row whose
grant counter changes from 0 to 2 instead of 1. A protected variant wraps receipt insertion
and update in one transaction with a unique event ID; the same replay must report no duplicate
application, rather than forcing the seeded-bug conclusion onto both versions.

If K2 loses its harness connection after the command was submitted, the director queries the
fixture's recorded command/event outcome before retrying. A reconnect must not submit the event
again and manufacture a duplicate. A missing Redis fixture is an environment blocker, not a
claim that retry safety passes or the application is broken.

Expected artifact: event order/crash point, transaction boundaries, final DB state and minimal
reproducer for each confirmed bug; separate missing-environment coverage. No purchases, real
subscription changes or production webhook calls.

Required: SWARM-08/16/19/41/43/47/52/53/55/58/64.

## S3 — Catalog: many-worker migration with shared-contract changes

Backend: TypeScript REST service with 24 resource modules, each using `page`/`offset` pagination.
Request explicitly authorizes implementation: “Migrate these endpoints to cursor pagination,
keep the documented response shape, and add behavior tests.” Fixtures include equal sort keys
and rows inserted between page fetches. Effective capacity is eight workers plus director.

The director first assigns one contract job for cursor encoding, stable ordering and response
metadata. The other modules wait on this dependency. After the contract is accepted, workers
take independent module jobs in waves of four, rising to eight if the first wave is healthy.
There are 24 module jobs plus contract/integration jobs, not 24 immediately running agents.

A worker discovers that sorting only by `created_at` loses rows with tied timestamps. It reports
the reproduction before completing its module. The director changes the contract to include
the primary-key tie-breaker and identifies all jobs referencing contract revision 1. Those
workers receive revision 2; already submitted revision-1 patches cannot enter integration
until revalidated. The contract repair is the second attempt of its existing logical job,
not a new identity that evades retry limits.

Workers return patches against isolated base revisions. Director integrates accepted patches
serially and runs combined tests for missing/duplicate rows and response compatibility. Eight
pending results pause new admissions; active workers may finish, so inbox length can briefly
exceed eight. Review drains it below four before more workers start.

Expected artifact: integrated branch in the task worktree, migration coverage per module,
contract revision history and test evidence. No automatic merge/push. Variants include a
conflicting patch, worker attempting a shared dependency upgrade, failure of combined tests,
and user narrowing scope to 12 modules. Preserve conflicting artifacts, require director
ownership for shared changes, reject failing integration, and supersede excluded work.

Required: SWARM-05/07/18–21/29/32/34/35/42–46/48/56/64.

## S4 — Dispatch: incident investigation with an account outage

Backend: Go shipment API, PostgreSQL, queue consumers. Request: “Using this exported trace/log
bundle, explain why shipment requests timed out between 10:00 and 10:15. Don't change services.”
Only the provided sanitized files and repository snapshot are authorized data sources.

Effective capacity: three workers plus director. L1 correlates request traces, L2 inspects SQL
and connection-pool behavior, L3 inspects queue retry paths. L1 reports a spike in pool wait
duration, and the director sends the trace IDs to L2 rather than forwarding every log line.
L2 finds a retry loop holding a transaction open; L3 checks whether message redelivery triggers
the same code path. One worker proposes “restart the database”; the director treats it as a
suggestion outside this read-only request and does not dispatch it.

L2's account becomes unavailable. The daemon preserves its note, source references and outstanding
question. Automode supplies another qualified allowed account; a replacement continues from the
checkpoint and consumes attempt 2. L1/L3 keep running. If the allowed replacement cannot receive
the log bundle, it is excluded. If all permitted accounts fail, the run blocks and stops model
wakeups instead of spinning through retries. The original run deadline remains in force.

Expected artifact: timeline, evidence-backed causal hypothesis, alternatives considered and
unverified assumptions. Exported logs alone may not establish causality; the director reports
that limit rather than upgrading correlation into proof. No live restarts, UI clicks or new
service integrations are implied.

Required: SWARM-02/06/14–16/28/30/34/41/47/51/52/57–59/64.

## S5 — Atlas under faults: make coordination fail deliberately

Replay S1 through scripted harnesses, each fault in a named deterministic test:

| Fault injection | Required observable response |
| --- | --- |
| Director dies after dispatch intent is committed but before worker acknowledgement | Reconcile by launch ID, retain one worker, and reject commands from the old director generation |
| Result persisted but receipt acknowledgement is lost | Worker replays the same message ID; one submitted result and one acceptance effect |
| Redirect delivered while worker is in a long tool call | Distinguish received from applied; hold conflicting/dependent work; checkpoint/interrupt after acknowledgement deadline |
| Worker keeps saying “working” without adding evidence | Heartbeats do not count as progress; bounded deadline/attempt policy yields stalled or failed, never a pass |
| PostgreSQL fixture shared accidentally by two workers | Detect conflicting resource ownership; invalidate contaminated evidence and rerun only within remaining attempts/budget |
| Director creates J8→J9→J8 dependencies or nonexistent J99 dependency | Reject the invalid subgraph before launching it; continue unaffected valid jobs |
| Storage fills while a worker submits evidence | Do not acknowledge durable receipt; hold new admissions and preserve/replay sender state after recovery |
| 2,000 duplicate progress messages arrive | Dedupe before model batching; apply bounded inbox/backpressure; Stop is not starved |
| API-token permissions revoked during work | Stop new dispatch on that identity immediately, interrupt affected work, and retain uncertain usage until reconciled |
| Account allowance drops because of external activity | Recompute headroom, stop new admissions as needed, and show the observed change rather than assuming exclusive use |
| Director and worker offer incompatible conclusions | Keep both evidence chains, perform one bounded reproduction if affordable, otherwise report unresolved |
| User presses Stop as the last worker submits | Preserve its artifact, honor cancellation ordering, and do not auto-resume or silently publish a completed verdict |
| User reduces worker ceiling below current activity | Drain to the new limit without starting replacements; never discard existing evidence |
| Run deadline expires while blocked | Checkpoint, cancel queued work, interrupt active work as supported; show incomplete with remaining coverage |

Required: SWARM-08/13/20–22/30/31/41–43/48–51/53/56/58/61–64.

## Scenario evidence requirements

Every scenario replay records fixture revision, policy snapshot, plan/assignment revisions,
declared accounts and units, actual dispatch counts, ordered message/receipt trace, resource
claims, final artifacts, acceptance decisions, and actual versus expected terminal state.
Do not mark a scenario passed from a model-written summary alone. Deterministic policy/fault
replays qualify the daemon behavior; separately authorized tiny live runs qualify each claimed
harness communication path. Neither substitutes for the other.
