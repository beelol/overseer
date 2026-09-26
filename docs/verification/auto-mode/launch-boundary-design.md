# Auto Mode durable launch boundary

Status: design and implementation plan; selected launch intent and pre-Git resource journaling are implemented, while the launch deadline and full crash reconciliation remain unverified. Date: 2026-09-26.

## Why this boundary is needed

`auto.dispatch` presently has a ten-second deadline for metadata collection and read-only preflight, then synchronously calls `delegate_run`. That call takes the parent workspace mutex without a deadline, takes and pins a Git snapshot, creates a Git worktree, writes the child and work-unit rows, emits a creation event, and starts the first turn. Git plumbing uses blocking `Command::output`. A client can wait beyond the decision deadline. More importantly, a daemon crash after a Git effect but before the work-unit row is committed can leave an unrecorded worktree. Repeating the same request may create a second child from a new selection. Merely returning a timeout from the caller while the launch continues would make the outcome uncertain.

## Required observable behavior

The request returns within ten seconds plus the one-second test tolerance with either a paused decision or a durable `launch_pending` handle. `launch_pending` means only that one launch intent has been admitted; it never reports a running child until the supervisor and turn state establish that fact. Concurrent and repeated calls with the same work-unit ID return that same intent and phase. Two distinct units may launch concurrently only when their allowance commitments and workspace ownership both fit. A manual writer competing for the parent workspace can prevent a launch; it cannot be bypassed by a later replay.

The launch worker may continue after the RPC returns. Its own Git, account-read, and process-start operations need bounded timeouts and a supervisor-owned execution budget. A timeout after an external effect is a reconciliation case, not proof that no effect happened. If the worker or daemon disappears, recovery first inspects the recorded intent, exact snapshot/worktree identity, workspace owner, child row, and supervisor state. It either resumes a proven unfinished phase once, returns the already-started child, or pauses with `effects_uncertain`. Recovery never launches a second process because a response was lost.

## Minimal durable phases and ownership

| Phase | Durable evidence before advancing | Permitted action on replay |
| --- | --- | --- |
| `selected` | Work-unit ID, requirements hash, parent run/checkpoint, route/account generation, selector version, and one decision event committed together | Revalidate before claiming; no Git or model effect has been requested |
| `claimed` | Allowance commitment and parent/workspace claim committed atomically with the selected intent | One worker owns preparation; competing units and manual writers see the claim |
| `preparing` | Exact snapshot ref and proposed branch/path identities journaled before each Git mutation | Inspect Git and database state; retry only an operation proven not to have occurred |
| `child_created` | Child/workspace/work-unit links and supervisor launch identity committed together | Reattach to that child; never create another for the same work-unit ID |
| `running` or settled | First turn and supervisor evidence, or a terminal result and released commitments | Return the existing handle/result; do not replay a turn or external action |
| `paused` | Reason and whether effects are known absent or uncertain | Require an explicit safe recovery action when effects are uncertain |

The named phases describe durable facts, not in-memory thread states. Phase transitions should be compare-and-swap or transactional so two daemon clients cannot both win. The work-unit ID and requirements hash remain the idempotency key. Journaled Git identities must be deterministic for the admitted work unit and validated against the expected parent snapshot before reuse; a coincidentally matching path or branch is not proof of ownership. Orphaned resources are reported, never silently deleted. A reservation is released only after confirmed pre-effect rejection or confirmed settlement, and recovery reconciles stale claims against real child/supervisor state. External account spending remains uncertain even when local reservations are exact.

## Implementation order and verification

1. Add a durable launch-intent record and protocol response for `launch_pending`, with request replay and two-client tests. Preserve the current manual delegation contract. The Auto UI and coordinator must display pending separately from running.
2. Move snapshot/worktree preparation behind the intent. Give Git launch operations bounded subprocess lifetimes and journal exact resource identities before each mutation. Test a stalled Git command, a process killed after worktree creation, and an ambiguous Git result. No alternate route starts in the ambiguous case.
3. Atomically attach the child and workspace to the intent, then start one supervised turn. Test crashes before the claim, after the claim, after worktree creation, after child-row commit, and after spawn; reconnect and a repeated request must never create a second child or second model request.
4. Integrate account-generation checks, available measured window commitments, shared-pool identity, competing manual writers, release on settlement, and recovery of stale claims. Test two distinct concurrent units against a known limiting window and a manual writer entering before launch.
5. Run the 100-candidate stalled-collector/launch test against the real daemon boundary and measure a concurrent UI request. Then exercise live authenticated routes separately where the RFC requires them. Fixture results cannot stand in for live account behavior.

Current focused tests cover metadata collection, pre-child rejection, child reattachment, and some idempotent replay. A controlled failed `git worktree add` demonstrates that a selected intent is persisted before that command and survives restart; replay and the manual delegation RPC do not attempt the Git operation again. A second controlled test lets Git create a worktree, then kills the daemon while its Git wrapper is still blocking before the child row can be written. The selected intent replays as effects-uncertain with the exact journaled path and branch and no second Git attempt. This tests one real Git-effect crash window but does not confirm ownership or automatically reconcile the orphan. Other crash phases, an atomic allowance commitment, a synchronous launch stall after selection, and the end-to-end ten-second response remain unproved. AUTO-AC-17, AUTO-AC-18, and AUTO-AC-24 therefore remain open.
