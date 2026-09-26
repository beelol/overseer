# RFC: category-directed adaptive Swarm mode

Status: draft product recommendation; implementation not authorized by this RFC.
Date: 2026-09-25
Scope: product behavior, integration boundaries, and acceptance criteria only.
Revision: director-led many-worker scope, continuous coordination, usable defaults, and
scenario-derived failure coverage. Supersedes the initial small-team framing.

## Purpose

Overseer should turn the user's available agents, accounts, and usage allowances into
useful completed work. Swarm decides whether a task benefits from delegation, how many
agents to run, and how to allocate work without consuming the capacity needed to finish.
More agents are not inherently better. A successful swarm may use one agent.

The intended experience is to give one director a category and objective, then let it
organize many subagents to complete the work. Working interpretation of **category**:
a named workstream such as backend, QA, or research, with its own scope, instructions,
backlog, and acceptance checks. This interpretation is proposed for review. A category
is not a provider, model tier, or mandatory repository boundary.

Concrete workflows, assignments, messages, expected artifacts, and injected failures are in
[the scenario specification](swarm-mode-scenarios.md). Those scenarios are part of this RFC;
the acceptance checklist in this file is the sole completion authority.

Workers normally inspect code, run commands/tests, and produce artifacts in their authorized
environment. VS Code is a control surface, not an implied test target. Starting a swarm never
closes the user's editor or runs Overseer's own UI/recovery test suite. Recovery tests below
are requirements for developing this feature, executed in isolated test environments.

The owner requested this RFC alongside separate automode and VS Code production-readiness
work. This document introduces no implementation changes and does not amend their acceptance
criteria. All numerical defaults below are proposed product policy, not previously approved
requirements or claims about provider limits.

## Product decision

Recommend **Start swarm** within a category, or a **Swarm** toggle on a task that creates
a category-scoped swarm run. Off remains the default. Each run has one logical director
and potentially many workers; independent jobs may use different harnesses/accounts.
The category is reusable, while each run has an explicit objective, budget, and completion
condition. Keep execution-target selection separate:

| Selection | Swarm off | Swarm on |
| --- | --- | --- |
| Manual target | Existing selected-target behavior | Divide work only within the explicitly allowed target pool; a single selected target remains a valid pool |
| Auto target | Automode routes one logical job | Swarm creates bounded jobs; automode routes each job within the same allowed pool |

Allow one active swarm run per category in v1; additional requests join its backlog or queue
for a later run. This keeps one director responsible for that category's active objective.

Swarm off means no Overseer-created delegation. It does not silently change existing
harness-native delegation behavior. Swarm on requires enforceable descendant limits, as
described below; observing children is not sufficient to control them.

The default objective is **balanced efficiency**: preserve required quality, then choose
parallelism only when its expected time benefit justifies additional usage and coordination.
Do not optimize for spending all available tokens or spreading work equally across accounts.

Alternatives considered:

- **Fixed agent count:** understandable and easy to bound, but wastes usage on serial tasks
  and cannot respond sensibly to outages or shrinking allowance. Retain a maximum as a limit,
  not a target headcount.
- **One combined Auto mode:** fewer controls, but opting into target routing would also
  authorize delegation and extra consumption. It also couples this feature to automode's UI.
- **Separate adaptive Swarm (recommended):** adds one control while keeping routing,
  delegation, and spending authority explicit. Advanced limits stay collapsed.

The first version has one balanced policy. Speed-first and conserve-first presets can follow
after evidence establishes useful differences; they are not hidden release requirements.

## User experience

Normal launch is **category + objective + Start**. Show a compact inherited-settings summary;
worker/model/account counts and budgets belong in optional advanced settings. Inherit previously
saved account choices; never silently include a newly discovered account. If none are allowed,
require a one-time account selection rather than repeatedly asking for every worker.

Show category, objective, scope, and director alongside those controls. The user can submit
a broad objective or an existing backlog; the director handles decomposition and dispatch
without requiring the user to launch every worker. Show ready, active, blocked, awaiting
review, and accepted jobs separately. A large swarm must remain inspectable through filters,
pagination, and per-worker details rather than requiring every transcript open at once.

After initial planning, show a compact, continuously updated explanation, for example:
“2 agents working; tests queued behind the implementation. A third worker would consume
the capacity reserved for review.” Distinguish active agents from total jobs and waiting agents.
Show actual target/account choices and whether usage is measured, estimated, stale, or unknown.

Controls are **Pause**, **Resume**, **Stop**, maximum active agents, and Swarm off. Pause
immediately prevents new dispatches and asks active runs to pause at supported checkpoints;
label any run still finishing its current turn. Stop cancels queued work and interrupts all
active descendants. Neither action claims success before acknowledgement or confirmed exit.
Turning Swarm off drains already-running jobs, prevents further delegation, and retains their
results for one-agent continuation. Lowering the maximum drains to the new limit without
discarding work. Stop remains available for immediate interruption.

User messages and changed requirements create a new plan revision. Completed work may be
reused only if still relevant; incompatible queued jobs are invalidated. Old workers cannot
silently overwrite a newer revision's accepted result.

### Defaults when the user chooses nothing else

These are proposed v1 defaults, now specified rather than deferred to implementation. Saved
category settings override saved application settings, which override built-ins; an explicit
run override wins over all three. Permission restrictions and stricter shared account/global
limits always constrain the result. Snapshot the effective settings and their sources per run;
changing a saved preset does not silently change an active run. Explicit changes to that run
take effect through a recorded revision. Permission revocations apply immediately.

| Setting | Built-in behavior |
| --- | --- |
| Accounts | Previously approved account pool; one-time selection if empty. Never use all discovered logins by inference. |
| Director and workers | Auto picks qualified targets from that pool. No model-name or harness-name preference embedded in swarm policy. |
| Worker count | Automatic, bounded by a default **8 workers per run** and **9 total executing agents globally**. These are adjustable ceilings, not launch counts or architectural limits. Account/machine limits can lower them. |
| Growth | At most 4 new worker admissions per wave, at least 5 seconds between growth waves. Failure, revocation and stop take effect immediately. A worker completion can refill within these bounds. |
| Run allocation | For every allowed shared pool/window with fresh comparable data, allocate at most **10% of its currently unreserved, unprotected remaining allowance**, less any tighter saved limit. Freeze that allocation for this run; a reset or newly found account cannot enlarge it. |
| Finishing reserve | Larger of 20% of the initial run allocation and estimated remaining synthesis/integration/verification cost, per applicable unit. |
| Unknown quota or uncalibrated job estimate | No automatic fan-out from that pool. Continue only an otherwise-authorized single-agent path under its existing limits; explain the limitation. Optional saved estimate-based permission can enable bounded fan-out. |
| Time | 60-minute run deadline, including waiting and recovery; checkpoint and stop at expiry. The user can extend the run explicitly. This is not a daemon shutdown or workspace deletion. |
| Attempts and repair | At most 2 execution attempts per logical job. At most 2 consecutive director turns without a plan change, accepted evidence, resolved blocker or other recorded material progress before entering a visible stalled state. |
| Review pressure | Pause new worker admissions at 8 submitted results awaiting review; resume below 4. Already-in-flight results are persisted even if the threshold is exceeded. |
| Routine messages | Director batches up to 20 envelopes or 32 KiB of inline summaries, whichever comes first. Wake when full or after 5 seconds; while a director turn is running, queue the next batch rather than starting a second director. |
| Context | Maximum 32 KiB inline worker brief, including injected coordination instructions, plus permission-checked artifact references. Fetch additional relevant context on demand; never silently truncate required constraints. |
| Job backlog | At most 1,000 nonterminal logical jobs per run; 100 ready jobs materialized at once, remainder durable. Expansion beyond the cap is blocked visibly pending an explicit limit change. |

The 10% default is an allocation policy, not a token conversion: 60 percentage points of
unreserved remaining weekly allowance yield at most 6 percentage points for this run. Every
binding window is checked separately. A worker still needs a compatible upper estimate before
admission. Without calibration, fall back as above; do not fabricate token-to-percentage rates.
Coordinator turns, messages requiring inference, verification, and retries consume the same
allocation. No background model calls just to ask whether anything changed.

## Boundary with automode and existing Overseer

Automode owns target eligibility/ranking and the acquisition/normalization of service health,
account authentication, quota windows, and model capabilities. This RFC proposes a consumer
contract, not a second telemetry implementation or a frozen API for the other agent.

Swarm owns decomposition, dependencies, active-agent limits, budget reservations, job
admission, retries, and integration/verification scheduling. The daemon owns this durable
state; closing VS Code does not reset reservations or stop the task.

Proposed logical exchange:

| Input/output | Required information |
| --- | --- |
| Target snapshot from automode | Stable target, harness, provider/model, account and shared quota-pool identities; scoped health/auth state; capabilities; observation time, expiry, provenance and confidence |
| Quota windows | Remaining amount and native unit, reset time if known, scope, whether exact/estimated/unknown; every binding window, not just the most favorable one |
| Job request to automode | Required capabilities/quality, allowed targets, estimated usage range, workspace constraints, exclusions after failure |
| Routing response | Eligible ranked targets or explicit unavailable/unknown reasons; snapshot version used |
| Admission by shared daemon authority | Atomic reservation of every applicable quota pool and concurrency slot, followed by launch; an expired or changed snapshot requires revalidation |

Reservation state must be shared across all Overseer tasks, including non-swarm launches.
Automode and swarm must agree on a single admission authority; two independent reservation
ledgers are unacceptable. Running work outside Overseer cannot be reserved by this daemon:
fresh provider observations can reduce headroom, and the UI must disclose that limitation.

Telemetry parsing, status-page interpretation, CLI probing, and any opt-in model-assisted
usage inference stay with automode. Swarm must not introduce background model calls to infer
quota. Inferred figures remain estimates and cannot establish a hard allowance guarantee.
No dependency on XCB is proposed; the existing source assessment records it as a previously
inspected, unselected candidate. This RFC makes no new claim about its current capabilities.

### Agetor orchestration comparison and reuse decision

Source inspected at **`bd5adcba1fd2c0df6d375fa43c9432294fe12dec`**. This is a targeted source
and test inspection, not an executed compatibility or performance benchmark. The earlier
repository assessment's README-only skip rationale is insufficient to dismiss these modules.
Keep Overseer's Rust foundation; port specific logic and tests only when they save effort.
A full fork or language-wide conversion is not a requirement.

| Inspected Agetor surface | What the source establishes | Required Overseer behavior |
| --- | --- | --- |
| [Task launch and orchestration](https://github.com/alamops/agetor/blob/bd5adcba1fd2c0df6d375fa43c9432294fe12dec/src/bun/orchestrator.ts) (`startTask`, `resolveHarness`) | Launch resolves the task's selected harness/profile; lifecycle handles start, follow-up, cancellation and reconciliation | Preserve explicit selection; add a category director and route each admitted worker through automode. No automatic quota-based account selection was found in the inspected launch path. SWARM-01/24/27/28. |
| [Usage providers and poller](https://github.com/alamops/agetor/tree/bd5adcba1fd2c0df6d375fa43c9432294fe12dec/src/bun/usage) | Per-harness quota snapshots, reset/scoped meters, API/cache fallbacks, refresh throttling and failure isolation | Potential automode reuse. Add shared quota-pool identity and atomic admission; preserve source/freshness/unknown distinctions. Undocumented interfaces need separate qualification. SWARM-08–14/24. |
| [Account usage accounting](https://github.com/alamops/agetor/blob/bd5adcba1fd2c0df6d375fa43c9432294fe12dec/src/bun/account-usage.ts) and its tests | Incremental Claude JSONL reading with persisted cursors, dedupe, partial-line handling and bounded scans | Preserve these cases if adapted; local history is observed consumption, not authoritative remaining subscription allowance. SWARM-09/12/13/38. |
| [Model discovery](https://github.com/alamops/agetor/blob/bd5adcba1fd2c0df6d375fa43c9432294fe12dec/src/bun/model-discovery.ts) | Probes use the selected harness's environment/binary and refresh on relevant state changes | Potential automode reuse; worker routing uses actual account capabilities, not a global model list. SWARM-06/24/38. |
| [Claude subagent tracking](https://github.com/alamops/agetor/blob/bd5adcba1fd2c0df6d375fa43c9432294fe12dec/src/bun/claude-subagents.ts) | Tracks native children/workflow containers/background work and completion receipts; some stale-state completion is explicitly inferred | Preserve descendant visibility and restart-safe receipt handling. Inferred silence never establishes accepted work or releases uncertain reservations. SWARM-17/19/31/38. |
| [Recovery tests](https://github.com/alamops/agetor/blob/bd5adcba1fd2c0df6d375fa43c9432294fe12dec/src/bun/reconcile.test.ts) and `session-liveness.ts` | Live-session reattachment, orphan handling, and distinguishing unreachable from gone; code is more capable than the README's kill-on-boot summary | Preserve surviving work and uncertain state; add director generation ownership and reservation reconciliation. SWARM-16/22/30/38. |
| [Agent profiles](https://github.com/alamops/agetor/blob/bd5adcba1fd2c0df6d375fa43c9432294fe12dec/src/shared/agent-profile.ts) | Reusable harness/model/effort/instruction/skill presets | Categories can supply defaults, but add explicit scope, backlog, acceptance and budget ownership; a launch preset alone is not a director. SWARM-27/28. |

No category director, adaptive worker-count policy, or shared account reservation scheduler
was found in these inspected paths. Do not generalize that finding into a claim that every
Agetor feature was audited. The goal is parity on the relevant lifecycle edge cases and
stronger explicit requirements for category-level coordination and budget-aware dispatch;
implementation is not yet proven equal or better.

Adapted modules must record pinned revision, copied/translated surfaces, MIT copyright/license
notices, and corresponding behavioral tests. Prefer structured harness APIs over terminal
heuristics where they meet the same need. Porting provider-specific parsing remains owned by
automode; this RFC does not start a competing collector effort.

## Director, category, and shared work

The director owns the category run's plan, job assignments, dependencies, acceptance decisions,
and final synthesis. Workers receive a bounded brief: category/run and plan revision, outcome,
scope and owned resources, relevant context/artifacts, target constraints, budget, and acceptance
check. They return artifacts, evidence, unresolved questions, and a concise handoff. They may
propose follow-up jobs; only the director admits them into the plan. Workers cannot silently
expand scope, change category, or recursively spend the category budget.

Use a durable shared work ledger for the plan, contracts, decisions, claims, artifact versions,
and acceptance evidence. Context sent to a worker contains only relevant decisions and sources,
not every peer transcript. A worker can request additional context through the director. Shared
information never bypasses the destination account/provider's context permissions.

The model director proposes actions; daemon scheduling and policy enforcement perform them.
Only one director generation may mutate a run's plan or assign work at a time, enforced with
a durable ownership lease and generation token. After director failure, reconcile its process
and retain workers/results before resuming it or installing a replacement. Reject all stale
director actions. Lease expiry alone does not prove the old process has stopped consuming quota.
If no director can recover, let bounded active jobs finish, queue their results, and pause new
dispatches. Never create a second independent decision-maker for the same run.

Workers report blockers and results to a durable director inbox. Batch routine completions
into one review turn; route stop requests, conflicts, and scope changes immediately to the
control layer. Results move from submitted to accepted/rejected with evidence; process exit
is not acceptance. A worker with stale/unreachable liveness becomes suspect or unknown, never
successful merely because it has stopped producing output. Retain late completion receipts without resurrecting
old work or accepting a result twice.

Changes to shared interfaces require a director-recorded contract revision. Workers dependent
on a superseded contract pause or replan. Separate categories have isolated ledgers and budgets
but share the global account reservation authority and workspace conflict checks. Cross-category
dependencies are explicit handoffs, not permission to consume another category's allowance.

### Communication protocol and redirection

The director stays informed through persisted events, not continuous model polling. The daemon
provides a broker and shared ledger across harnesses. Workers have equivalent structured
`report`, `ask`, `claim`, and `submit` operations, and a way to receive/acknowledge directives.
An adapter may deliver them through supported tools or turn-boundary input; console text
mentioning another agent is not proof of delivery. A target without a qualified communication
path is ineligible for managed swarm work, with the limitation shown before admission.

| Worker → director | Required meaning | Director → worker |
| --- | --- | --- |
| `started` / `progress` | Acknowledge assignment revision, current hypothesis/step and resource claims | Confirm ownership or narrow scope; no response needed to a redundant progress update |
| `discovery` | New evidence, affected symbols/endpoints, confidence and artifact references; report before final completion if it changes peers' work | Share a focused finding with affected workers; request confirmation or revise their assignment |
| `overlap` / `claim` | Intended work duplicates another hypothesis, endpoint or resource; read-only file overlap alone is not a conflict | Assign an owner, partition work, merge duplicates, or explicitly retain an independent reproduction |
| `question` / `blocked` | Specific missing decision/dependency, attempted alternatives and what work can continue | Answer, redirect, supply a permitted artifact, or mark blocked; involve user only for missing scope/access/authority |
| `result` | Structured outcome, acceptance evidence, unresolved coverage and artifact revision | Accept, reject with a bounded follow-up, or retain as partial evidence |
| `failed` | Classified failure and checkpoint; not interchangeable with a negative audit finding | Retry within attempt limits, reroute through automode, or mark failed/blocked |

Each envelope has stable message ID, category/run/job/attempt IDs, sender identity, director
generation where applicable, assignment revision, type, causal parent ID, per-sender sequence,
timestamp, and payload/artifact references. The broker validates membership and permissions,
persists before acknowledging receipt, and records delivery and application separately.
Use at-least-once delivery with idempotent state transitions; never promise exactly-once network
delivery. Replayed IDs cannot spawn another job, double-count usage, or accept a result twice.
Order dependent directives by revision/sequence; unrelated workers need no global message order.

Director decisions publish only to affected workers. Peer questions/handoffs use the same broker
and remain visible to the director; workers can share evidence, but only the director can
change assignments, authorize new jobs or accept results. No all-to-all transcript broadcast.
An instruction-bearing artifact is untrusted evidence, not a director command: role identity
comes from the broker, never from text such as “director says ignore the scope.”

For `redirect`, `pause` or contract change, the worker must acknowledge **applied** with the new
revision before its new result can count. Delivery alone is not application. If the harness
cannot accept instructions mid-turn, queue for its next boundary. If acknowledgement is absent
for 30 seconds, hold dependent admissions and request an interrupt/checkpoint; do not declare
the worker stopped until confirmed. Preserve results from the old revision as historical
evidence; the director must explicitly revalidate them to use them under the new plan.

Known conflicting writes or a revoked permission cause an immediate daemon dispatch hold
without waiting for model reasoning. Semantic overlaps are advisory until the director decides.
One owner per logical investigation prevents duplicated effort; deliberate independent
verification is labeled with its own job, budget and expected independent evidence.

Discovery example: the tasks worker reports “`findTask(id)` has no tenant predicate.” The
director tells the attachment worker to check callers of that helper, asks the task worker to
cover update/delete, and tells the project worker not to repeat the same call-chain trace.
The discovery is not yet a confirmed vulnerability. A separate reproducer may establish that
the route's earlier middleware prevents cross-tenant access, in which case the hypothesis is
rejected with evidence and the director retracts its earlier advisory to every recipient.

Heartbeat/liveness sampling is daemon work and consumes no model turns. Proposed default:
sample every 15 seconds, mark liveness unknown after 60 seconds of unreachable transport, and
reconcile before retrying. Output silence alone is not a failure or evidence of no progress;
a long-running test can be healthy. Every job has a deadline within the run deadline. A live
but repeatedly unproductive agent is limited by the attempt/no-progress policy, not by invented
success. Two failed planning/repair turns block planning; they cannot recursively spawn planners.

### Persisted states and unexpected input

Jobs move through `planned → ready → reserved → launching → running → submitted → accepted`.
Rejection can produce the second execution attempt, never an unlimited review loop. Side states
are `blocked`, `suspect`, `failed`, `cancel_requested`, `cancelled`, and `superseded`. Keep attempt
and process records separate from logical job records: submitted output may arrive before a
process exits, and accepted artifacts do not prove its usage has ended. Release budget/slot
reservations only on confirmed non-consuming state; accepted results unlock dependents only
when there is no conflicting live writer.

Reject cyclic dependency graphs, unknown dependency IDs, conflicting exclusive claims, and
missing acceptance checks before dispatch. Preserve independent valid jobs if possible. Reject
malformed/oversized messages with a bounded diagnostic; artifacts hold large payloads. At a
1,000-envelope director-inbox high-water mark, stop new workers and ask active ones to checkpoint;
do not drop terminal events. The broker withholds receipt acknowledgement for anything it cannot
persist, so senders retain/replay it. Storage failure stops new admissions and surfaces a blocked
run. Recovery replays committed state; no acknowledgement may imply an uncommitted durable result.

Contradictory findings keep both evidence sets. The director assigns bounded reproduction work
or reports unresolved disagreement; majority vote or a worker's confidence is not verification.
Missing required services/access produces blocked coverage, not a pass. Stale source revisions,
missing artifacts, test contamination, or environment failures invalidate the affected evidence.
Workers distinguish test-environment errors from application defects and avoid silently retrying
side-effecting commands. All-blocked runs stop model wakeups until a relevant event or user input;
the run deadline still applies.

## Scheduling and agent quality

The director (the lead agent) proposes an incrementally expandable dependency graph. Every job needs an outcome, acceptance
check, dependencies, owned files/resources, required capabilities, usage estimate, and a
bounded completion condition. Decomposition itself consumes the task's budget. Use a bounded
planning turn; an invalid plan falls back to serial work or a visible planning failure.

Only ready, independent jobs are candidates for parallel execution. Work that changes the
same interface or depends on another job's findings remains serial. A dependency graph may
contain many queued jobs; that does not justify launching them all.

The scheduler is deterministic given a validated plan, telemetry snapshot, and policy
version. Model reasoning may propose a plan and estimates, but cannot override limits.
Consider serial execution first, then feasible parallel batches. Estimate both makespan and
total usage, including planning, context transfer, workers, retries, integration, and review.
Admit an extra worker only with a recorded expected time benefit, affordable total usage,
and adequate finishing capacity. When estimates do not justify parallelism, choose serial.
Stable ties prefer fewer agents, then saved user target preference, then stable target ID.
Record estimates and actual outcomes to support later calibration; do not advertise global
optimality or guaranteed token savings.

Assign the least costly eligible target that meets the job's quality requirements under the
available evidence. “Small model” is not itself a quality qualification. Use bounded workers
for well-specified independent work; keep a sufficiently capable lead for integration and
ambiguous decisions. Verification checks the combined result. It need not always use a
separate agent or provider, but must actually execute the specified checks.

Proposed concurrency policy:

- **Auto-sized worker pool**, with no fixed three-agent product ceiling or default. Compute
  current worker admission from ready independent work, approved allocation, provider/account
  concurrency, machine capacity, and the user/global maximum. Show the effective maximum and
  limiting factor before worker dispatch. Built-in ceilings supply a bound when no saved limit
  exists; unknown telemetry still cannot establish affordable capacity. A qualification target of **32 concurrent
  workers plus one director and 100 logical jobs** proves many-worker support with fixtures;
  it is neither the default live launch size nor an assertion that an account supports it.
- Every executing director, worker, reviewer, and native descendant counts toward the shared
  global active-agent limit. Reserve one execution slot for each admitted swarm's director so
  full worker capacity cannot prevent coordination. A waiting director does not count as active,
  but its reserved slot cannot be borrowed by a worker. A total limit of one uses the director
  for serial work. A run awaiting a director slot remains queued.
- Increase concurrency in bounded waves as compatible capacity and useful work become available;
  reduce new admissions when limits shrink, failures increase, or review/integration backs up.
  Record the policy's wave size, queue thresholds, and cooldown in the run snapshot. Do not
  start all queued jobs merely because the director created them. Resume useful admissions
  after a confirmed recovery without repeatedly interrupting healthy workers.
- Overseer schedules one worker level beneath the lead. Recursive autonomous delegation is
  disabled unless the adapter can enforce the same total limits and report every descendant.
- Each logical job receives at most **2 total execution attempts** across all targets.
  Reconnection to the same confirmed run is not a new attempt. Replanning cannot reset this
  counter for the same logical work; materially new work gets a recorded new job identity.
- Replan on job completion/failure, material telemetry changes, or user changes. Coalesce
  repeated events. Do not repeatedly ask a model to replan while nothing has changed.

Bound the ready queue, result inbox, event batches, and per-worker context in a versioned
policy. Excess work remains in the durable backlog; it is not dropped or loaded into one
unbounded prompt. Stop producing workers when their pending results exceed the configured
integration/review capacity. Support reuse of a worker session for a related follow-up when
its context is valid; unrelated jobs receive fresh context. Use round-robin admission across
eligible categories by default, with explicit priority overrides; no category monopolizes
newly available shared capacity merely by generating the largest backlog.

## Budget semantics

Within Swarm mode, the task allocation below is the category run's total allocation, shared
by its director and all jobs/attempts. Every child draws from that same allocation; creating
a job, follow-up, or replacement director never creates a fresh budget.

Keep budgets in their real units. Tokens used are not tokens remaining. A subscription
percentage is not a token count. Do not sum unrelated percentages or assume model/provider
tokens have equal cost. Multiple profiles or harnesses for one subscription share a quota
pool; switching from one harness to another does not replenish that pool. If independence
cannot be established, conservatively group possibly shared capacity rather than double it.

For each metered pool/window, admission requires:

`fresh usable remaining - existing reservations - new job upper estimate >= protected capacity`

Apply every relevant limit: account/model quota windows, per-task budget, global active-agent
limit, and target/local-resource concurrency. Where units cannot be reconciled, do not invent
a conversion. Use an explicit user-supplied bound in a supported unit or treat that pool as
unknown for automatic fan-out.

Protect the larger of **20% of the task's initial allocation** in that unit and the estimated
remaining integration/verification cost. Account capacity the user separately protects for
other work is unavailable before computing the task allocation. The completion reserve is
not a second allowance: only finishing jobs may spend it. Never silently lower the reserve
to keep workers busy. If finishing cannot fit, preserve results and report blocked/incomplete.

Task allocations must be explicit or deterministically derived from fresh compatible quota
data and the defaults/saved user limits above, and visible before worker admission. A window reset refreshes
eligibility only after new evidence; it does not increase a task's approved allocation.

With unknown/stale quota, default to no automatic fan-out from that pool. An already-authorized
single-agent run may continue under its existing limits. A user can explicitly permit a
bounded estimate-based allowance; display the uncertainty and retain time/attempt/concurrency
bounds. This permission cannot be inferred merely from turning Swarm on.

Reservations are upper estimates, not provider billing enforcement. A strict token/spend cap
is offered only where the adapter can enforce it, including descendants and in-flight work.
Otherwise label it an estimate-based stopping threshold with possible overshoot. On detected
exhaustion or overrun, stop new admissions, interrupt affected runs as supported, and reconcile
reported usage. Uncertain launch/termination retains its reservation until reconciled; do
not free quota while a process may still be consuming it. Avoid counting inclusive parent
usage and child usage twice; preserve unattributed usage explicitly.

## Failures, work ownership, and recovery

An outage, expired login, quota exhaustion, rate limit, and local harness failure are different
events with different scopes. Swarm follows automode's scoped eligibility; it does not use
provider-name if/else chains. An OpenCode provider failure does not imply all OpenCode targets
are unavailable. A healthy public status page does not prove that a specific account works.

Keep healthy jobs running when another target fails. A replacement consumes an attempt and a
new reservation. Resume on the same harness only when supported; switching harness/provider
uses a portable checkpoint (intent, constraints, accepted artifacts, outstanding checks, and
provenance), not a claimed transfer of a native conversation. Transfer only context permitted
for the destination account/provider. If no permitted target qualifies, queue/block with a
reason; do not substitute a weaker model, buy credits, or change login state.

Never retry uncertain external side effects automatically. Reconcile their outcome first.
No speculative duplicate races in v1. Default to one writer per isolated worktree; workers
return patches/results with base revision and ownership. Integrate accepted changes serially
into the task's isolated integration worktree, check conflicts, then verify the combined
result. Do not auto-merge to the user's branch, publish a PR, delete worktrees, or overwrite
dirty user work. Current-checkout mode permits at most one writer; concurrent read-only work
requires a stable snapshot. Preserve both versions on conflict and surface required action.

Native children must either participate in enforceable slot/budget accounting or be disabled
through a verified harness capability. A prompt saying “do not spawn” is not enforcement.
If neither is possible, that target is ineligible for managed swarm execution, while remaining
available for ordinary manual use. Surface this limitation before dispatch.

Persist plan revisions, logical jobs, attempts, reservations, checkpoints, and launch identity.
After a daemon restart, reconcile surviving processes before replaying dispatch. Use durable
launch intents and stable identities so a crash between launch and recording cannot create
a second worker. Never report completion just because workers exited: required job acceptance
checks and combined-result verification must pass. Report failed, blocked, stopped, and
partially complete outcomes accurately.

## Scope and delivery boundaries

In scope: category-directed delegation, shared admission, explainable adaptation, safe result
integration, and category-run/worker controls. Existing account-only authentication and approval rules still apply.

Out of scope: new harness adapters, new quota collectors, API-key fallback, purchases, account
creation, automatic publishing/merging, speculative races, continuous background work, TUI,
and VS Code production-readiness fixes. Separate future authorization is needed for implementation
and live model tests. Do not inherit an earlier agent's time/token budget as product policy.

Implement later in dependency order: agree automode contract and control capabilities; verify
deterministic scheduler/reservations with fixtures; add isolated execution/recovery; add UI;
qualify actual target/account combinations with minimal live runs. Missing telemetry/control
capabilities block the affected acceptance claim, not an excuse to create competing adapters.

## Acceptance criteria

All criteria are initially unchecked. Use the `SWARM-` namespace so this checklist cannot be
confused with the base product or automode. Each row is a required observable scenario; a
fixture proves policy behavior, not live provider compatibility.

| ID | Required outcome and verification |
| --- | --- |
| SWARM-01 | [ ] Exercise all four Auto/Manual × Swarm on/off combinations. Swarm off creates no Overseer worker; manual swarm stays within its selected pool; enabled swarm may choose one agent. Existing manual/native behavior remains intact. |
| SWARM-02 | [ ] A newly discovered account remains excluded until selected. A denied provider/context destination is never used for workers, fallback, or checkpoint transfer; test routing with only denied candidates. |
| SWARM-03 | [ ] Replay a serial dependency chain, three independent jobs, and overlapping write ownership. Only ready independent jobs run concurrently; serial execution has an explicit reason. |
| SWARM-04 | [ ] Given identical plan, policy, telemetry, and reservation state, scheduler decisions and reason codes match on replay. Changing a provider name without changing capabilities/health does not change policy eligibility. |
| SWARM-05 | [ ] In paired serial/parallel fixtures, include planning/context/integration/review costs. Reject parallelism when benefit is absent or finishing becomes unaffordable; admit an affordable beneficial batch. Retain estimates and actuals. |
| SWARM-06 | [ ] Give a cheap target insufficient capabilities and a qualified target higher cost. Reject the cheap target; if no qualified target exists, block visibly instead of degrading the quality requirement. |
| SWARM-07 | [ ] With a configured total max=3, reserve one director slot and allow at most two simultaneous workers. Count reviewers/native descendants too. Demonstrate no fourth admission, director activation while both workers run, slot reuse after confirmed completion, and draining after max is lowered. Max=1 performs serial director work. |
| SWARM-08 | [ ] Concurrent swarm and non-swarm launch requests against one nearly depleted pool cannot both reserve the last capacity. Repeat across two tasks and two profiles known to share a subscription; prove atomic admission. |
| SWARM-09 | [ ] Two harnesses using the same account share capacity; verified independent accounts retain separate capacity. Uncertain identity cannot produce a doubled allowance. |
| SWARM-10 | [ ] With 100 compatible units allocated, a 20-unit reserve and 10 already reserved, admit a 60-unit worker and reject a further 11-unit worker. With finishing estimate=35, reject that 60-unit worker. Allow finishing work to draw on the reserve. |
| SWARM-11 | [ ] Test conflicting short/long quota windows, unlike units, and a reset. The most restrictive applicable limit binds, percentages are not added or converted without evidence, and reset does not enlarge approved task allocation. |
| SWARM-12 | [ ] Unknown, stale, and inferred quota never appears as zero or unlimited and cannot enable default fan-out. Explicit bounded estimate permission changes eligibility with uncertainty visible; revocation stops new admissions. |
| SWARM-13 | [ ] Demonstrate actual adapter enforcement before labeling a limit strict. For delayed usage/overshoot fixtures, show estimate-based labeling, stop admissions on overrun, retain uncertain reservations, and reconcile without double-counting parent/child totals. |
| SWARM-14 | [ ] Inject provider outage, account auth failure, quota exhaustion, rate limit, and local harness failure independently. Only affected targets are excluded; unrelated healthy jobs continue. OpenCode with a second healthy provider remains eligible if qualified and allowed. |
| SWARM-15 | [ ] Repeated routing failures across multiple targets produce no more than two execution attempts per logical job. Replanning cannot reset the attempt budget; an unchanged waiting state produces no repeated model-planning calls. |
| SWARM-16 | [ ] After worker failure, recover accepted artifacts through a checkpoint on another eligible target without claiming native session portability. An uncertain external side effect blocks retry until reconciliation. |
| SWARM-17 | [ ] For each enabled harness, either prove descendant admission/control within the total limits or prove native delegation disabled. Observe an attempted unmanaged spawn. Unsupported control makes the target visibly ineligible for swarm. |
| SWARM-18 | [ ] Independent writers use isolated worktrees; current-checkout mode admits only one writer. Inject overlapping patches and pre-existing dirty/unsaved user work; preserve both sides and never silently overwrite, publish, or merge to the user's branch. |
| SWARM-19 | [ ] Workers pass individually but their combined output fails a check. Task remains incomplete until integration and required verification pass; stopping or exhaustion preserves partial artifacts without reporting success. |
| SWARM-20 | [ ] Pause, Resume, Stop, and Swarm off behave as specified with active and queued jobs. Stop reaches descendants; unconfirmed exits remain visible and retain reservations. Swarm off prevents new delegation while draining active jobs. |
| SWARM-21 | [ ] Change user requirements during execution. Invalidate incompatible queued jobs, retain revision provenance, and reject stale worker results from automatic acceptance into the revised plan. |
| SWARM-22 | [ ] Close/reopen VS Code and crash/restart the daemon before launch, after launch but before acknowledgement, and after result receipt. Reconcile without duplicate workers, lost accepted results, or freed reservations for surviving work. |
| SWARM-23 | [ ] UI evidence shows active/queued counts, selected targets/accounts, limiting constraint, measured/estimated/unknown usage, finishing reserve, and an explanation for serial, scaled-down, and blocked decisions. Logs omit credentials and unnecessary raw account output. |
| SWARM-24 | [ ] Consume the agreed automode interface and single shared admission authority. Contract tests cover expired snapshots and changed eligibility between routing and launch; no duplicate health/quota collector is introduced. |
| SWARM-25 | [ ] Publish version-pinned evidence per supported harness/account/platform for a minimal two-worker task, one-agent fallback, cancellation, and recovery. Keep unsupported or credential-blocked combinations explicitly unverified; do not represent fixture coverage as live coverage. |
| SWARM-26 | [ ] Compare serial and swarm on a fixed suite containing serial, independent, conflicting, and constrained-budget tasks. Record acceptance pass rate, elapsed time, usage in native units, and overhead. All deterministic budget/safety scenarios pass; at least one suitable parallel case improves elapsed time at equal acceptance quality within its approved allocation. Do not require every task to benefit or claim universal savings. |
| SWARM-27 | [ ] Start a named category swarm from an objective and from a supplied backlog. One director decomposes, assigns, reviews, and synthesizes without per-worker user launches. All jobs retain category/run identity, scope, plan revision and acceptance checks. Completion requires the objective's checks, not an empty active list. |
| SWARM-28 | [ ] Independently runnable jobs needing different capabilities route to different qualified, allowed accounts/harnesses when appropriate. Worker proposals outside category scope or budget cannot dispatch themselves. A manual single-target pool remains respected even when a better external account exists. |
| SWARM-29 | [ ] Run 100 fixture jobs with sufficient compatible allowance, independent work, a total limit of 33, and a worker limit of 32. Demonstrate 32 active workers plus director capacity, no oversubscription/duplicate jobs, and all 100 accepted exactly once after checks. Then constrain capacity and show a smaller effective pool with an explanation. No paid 32-agent run is required. |
| SWARM-30 | [ ] Crash the director while workers run; preserve their results and resume/replace the director using the durable plan. Reject a stale director's dispatch and acceptance requests after replacement. Simulate uncertain termination and unavailable replacement; retain reservations and pause new jobs without discarding results. |
| SWARM-31 | [ ] Replay a native asynchronous launch stub, a completion receipt, duplicate receipts after restart, silent/stale output, and a late output append. A stub is not completion; silence is suspect/unknown; completed work is not resurrected by replay; acceptance and accounting occur once. Include a parent finishing before its descendants. |
| SWARM-32 | [ ] With fixture review capacity of 8 pending results, a wave size of 4, and 40 ready jobs, stop new admissions once review backlog reaches 8; accept already-in-flight results durably and drain them before resuming. Lower capacity during an outage, recover it, and verify bounded waves and no repeated replanning on unchanged events. |
| SWARM-33 | [ ] Batch 20 routine completion events into one director review turn under the fixture's batch policy. A Stop event prevents new dispatch immediately without waiting for that model turn. Persist events through restart and reject stale plan revisions; no result is dropped or processed twice. |
| SWARM-34 | [ ] Inspect worker briefs and director summaries for a 100-job fixture. They obey configured byte/token bounds and contain relevant artifact references rather than all peer transcripts. Context requests recover omitted detail subject to destination permissions; large artifacts remain retrievable. |
| SWARM-35 | [ ] Two workers depend on one interface contract; changing it invalidates the affected pending result/job and blocks stale integration. A rejected result cannot unlock dependent work. Reuse a valid related worker session, but do not carry unrelated category context into a new job. |
| SWARM-36 | [ ] Run two eligible categories with backlogs of 100 and 2 jobs against shared accounts and enough budget. Under default round-robin admission, each category receives one of the next two available worker admissions when its director slot is reserved. Preserve separate scopes, ledgers, and budgets; cross-category resource conflicts/handoffs remain explicit. |
| SWARM-37 | [ ] In a UI fixture with 100 jobs and 32 active workers, display director, aggregate counts, blockers and account usage; filter by state and inspect one worker without mounting every transcript. Pause/Stop commands are acknowledged by the daemon within 2 seconds on the recorded test machine, with unconfirmed process exits shown separately. |
| SWARM-38 | [ ] Publish the Agetor parity/reuse ledger for the pinned revision above: selected behavior, corresponding Overseer test/evidence, intentional differences, and attribution for anything ported. Cover partial transcript lines, duplicate usage, unavailable quota, account-specific discovery, live-session recovery and descendant completion ordering. Unimplemented/unverified rows remain gaps, not claims of superiority. |
| SWARM-39 | [ ] Replay S0 with advanced settings unset: inherit the approved pool, automatically choose qualified targets, apply the 8-worker/9-global ceilings and documented defaults, and start without per-worker questions. Empty account permission triggers only the needed initial selection. Verify run > category > application > built-in precedence, unchanged active policy after preset edits, and immediate permission revocation. |
| SWARM-40 | [ ] With 60 percentage points of fresh unreserved/unprotected remaining weekly allowance and no override, derive at most 6 points for the run and a minimum 1.2-point finishing reserve. Apply a tighter short-window limit independently. Missing compatible worker estimates prevents default fan-out; no token conversion is fabricated. Planning, routing inference, messaging, reproduction and replacements share the original allocation. |
| SWARM-41 | [ ] In S1, deliver D1 before J2 completes. Persist it, notify the director in the next eligible bounded batch, route the relevant advisory only to J1/J4, and record their receipt/application. An in-flight director turn queues the next batch instead of creating another director. No periodic model calls occur in an unchanged run. |
| SWARM-42 | [ ] Duplicate, delay and reorder progress, discovery, result and acknowledgement messages. Stable IDs and revision checks yield one effect per event; no double dispatch, double acceptance or accounting. Crash between persistence and acknowledgement, then replay; result survives. Unknown identities, cross-run messages and stale director generations are rejected. |
| SWARM-43 | [ ] Deliver J4's revision-2 redirect while its tool call is active. Show delivered versus applied separately; old-revision output cannot satisfy the new assignment. After 30 seconds without applied acknowledgement, hold dependent work and request interrupt/checkpoint. A harness lacking any qualified delivery/acknowledgement path is excluded before swarm launch. |
| SWARM-44 | [ ] In S1, J2/J4 claim the same investigation and J1 reads the same file for a different question. Director assigns one owner for the duplicate hypothesis, redirects the other, and permits independent read-only analysis. Conflicting exclusive write/resource claims are held atomically across runs/categories. |
| SWARM-45 | [ ] J7 is explicitly created as independent reproduction with its own bounded job and budget; duplicate discovery messages cannot create repeated reproducers. Director can merge duplicate findings without losing endpoint-specific evidence or counting agreement as proof. |
| SWARM-46 | [ ] Change the shared pagination contract in S3. Notify every affected active/pending job, require revision acknowledgement and invalidate affected stale results; preserve unrelated accepted work. Retract D1 in S1 and propagate the correction to all prior recipients. No silent use of withdrawn conclusions. |
| SWARM-47 | [ ] Feed the director conflicting J2/J7 evidence. Retain both chains and require bounded reproduction or an explicit unresolved outcome. A confident claim, majority agreement or empty worker exit cannot become a confirmed finding without required evidence. |
| SWARM-48 | [ ] Validate plans containing cycles, nonexistent dependencies, duplicate logical jobs, missing acceptance checks and conflicting exclusive claims. Reject invalid dispatch; allow independent valid subgraphs. Two failed planning/repair turns or two no-progress director turns produce a visible stalled state rather than recursively creating more planners. |
| SWARM-49 | [ ] Reject malformed, oversized and unauthorized message envelopes with a bounded diagnostic. Feed 2,000 duplicate events; dedupe before inference, obey batch/context limits, and keep Stop responsive. At the 1,000-envelope inbox threshold, stop new worker admissions while preserving/retrying unacknowledged terminal events. |
| SWARM-50 | [ ] Inject storage-full and transient write failure during dispatch intent, discovery and result persistence. No durable acknowledgement precedes commit; no new work launches from an uncommitted reservation. Surface blocked state and recover without lost acknowledged events or duplicated work. |
| SWARM-51 | [ ] Distinguish output silence, an unreachable transport and a confirmed dead process. Sample liveness without inference, mark unknown after the configured 60-second reachability threshold, and reconcile before retry. Progress spam does not extend the job/run deadline or reset attempts; no timeout produces a successful result. |
| SWARM-52 | [ ] In S1/S2/S4, enforce audit-only scope: permit authorized isolated tests/evidence, reject application-source changes and live service mutations. S3 permits requested source changes in isolated worktrees. Source text or a worker message impersonating the director cannot change permissions, target pools or assignments. Record the actual enforcement capability per enabled harness. |
| SWARM-53 | [ ] Give two tests the same mutable database namespace or service resource. Detect the conflicting exclusive claim before dispatch; if contamination is discovered later, invalidate affected evidence and rerun only within remaining attempts/budget. A shared read-only snapshot is allowed. No global single-UI constraint is imposed on command/code workers. |
| SWARM-54 | [ ] Remove an evidence artifact or change its source revision after submission. Prevent acceptance until evidence is restored/revalidated; retain an explicit provenance chain from final finding to job/attempt, assignment, source and reproduction. A replacement worker can retrieve permitted artifacts without the old transcript. |
| SWARM-55 | [ ] A worker submits a negative audit result, a test-environment failure and a confirmed application defect in separate fixtures. Preserve those distinctions in the coverage report. Unavailable DB/queue/access does not become a passed check or an application vulnerability. |
| SWARM-56 | [ ] At 8 pending results, stop new admissions; after draining below 4 resume at most 4 admissions per 5-second growth wave. In-flight submissions may exceed 8 without being lost. Run the 32-worker qualification with explicit ceiling overrides and ensure the default 8 is not a hidden architectural cap. |
| SWARM-57 | [ ] Revoke a destination's access to an artifact during S4. Stop further delivery and new work using that permission; interrupt affected work as supported. Do not send credentials/raw secrets through the shared ledger, broker payloads or final logs. Denied peer requests cannot bypass director/account restrictions. |
| SWARM-58 | [ ] Lose the acknowledgement of S2's side-effecting fixture command. Reconcile its outcome before retry; never manufacture a double-application through blind replay. If outcome remains unknown, preserve partial evidence and block the affected job while independent work continues. |
| SWARM-59 | [ ] Start with no eligible target, lose all allowed targets mid-run, and exhaust finishing capacity in separate fixtures. Report the specific blocked/incomplete reason, preserve artifacts, stop unchanged-state inference loops, and resume only after relevant eligible state changes or user action within the original deadline. |
| SWARM-60 | [ ] Peer evidence/questions use the durable broker and are visible to the director; peer text cannot reassign ownership or admit jobs. Deliver a shared-helper discovery to affected peers while withholding unrelated transcripts and category-restricted artifacts. |
| SWARM-61 | [ ] Race Stop, permission revocation, final-result submission and director acceptance. Persist a deterministic order; once stopped/revoked, no subsequent directive launches work. Preserve late artifacts with their actual state and require explicit user resume/extension; never auto-resume from a worker event. |
| SWARM-62 | [ ] Expire the default 60-minute deadline while active and while blocked. Checkpoint, cancel queued work, interrupt active work as supported and show unconfirmed exits. Do not kill unrelated sessions or close VS Code. An explicit extension is recorded and does not silently enlarge account allocation. |
| SWARM-63 | [ ] An ordinary backend swarm runs without testing/restarting Overseer, controlling the editor UI, or requiring one UI per worker. Validate actual shared-resource requirements instead. Execute SWARM-22/37 only in isolated product-test environments, not as side effects of user swarm runs. |
| SWARM-64 | [ ] Replay S0–S5 with versioned fixtures and expected message/dispatch/artifact traces. Verify the stated outputs and each injected fault, including default launch, scope/overlap coordination, account replacement and partial coverage. Separate deterministic replay evidence from version-pinned live communication-path qualification under SWARM-25. |

## Evidence and future goal

For each criterion, a future evidence record must contain status (`unverified`, `verified`,
`failed`, `blocked`), exact revision, policy and fixture versions, input snapshot/plan, expected
and observed behavior, reproducible command or UI steps, and evidence paths. Live records also
need harness version, account path (redacted), platform, and actual usage when exposed. Checkbox
completion requires the entire row to be verified. A blocker is not a pass.

Suggested evidence location: `docs/verification/swarm/SWARM-XX.md`; create it during the future
implementation, leaving the base product ledger untouched. Run deterministic tests for fault,
retry, quota, and concurrency loops. Use only explicitly authorized tiny live smoke runs;
do not loop paid model benchmarks to satisfy the checklist.

The [prepared implementation goal](swarm-mode-goal.md) targets **SWARM-01–64** and S0–S5.
It is not active. Approving this RFC does not itself run agents, create an automation, or
authorize an implementation session's duration, budget, publication or live account tests.

## Review points and local grounding

The recommended decisions for owner review are the separate opt-in toggle, balanced objective,
category-scoped director, continuous brokered coordination, adaptive many-worker capacity,
the defaults table (including allocation/deadline), and conservative handling of unknown quota.
Implementation planning should follow approval of this RFC, not proceed automatically.

Repository context inspected at `19edf57` (2026-09-25):

- [Base RFC](../overseer-rfc.md): daemon ownership, account-only scope, isolated workspaces,
  native-child observation, and routing/delegation deferred to separate work.
- [Compatibility record](../compatibility.md): recorded evidence varies by harness/account;
  child observation and usage reports do not establish enforceable scheduling limits.
- [Source assessment](../source-assessment.md): prior reuse assessment, including XCB.
- [Durable records](../../daemon/src/store.rs) and [adapter capabilities](../../daemon/src/adapters.rs):
  existing integration seams; this draft changes neither file nor their interfaces.

No external service capability was newly verified and no live harness test was run for this RFC.
