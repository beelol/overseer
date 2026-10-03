# Audio semantic producer boundaries

Source-only supplement to the approved twelve-line plan. Base79a334cd/990d784a;
actual shared baseline038c2c41; canonical Line APIed947e5. No runtime qualification
of the new API or classifier exists yet. This maps decisions before the runtime
replacement; it does not redefine the owner spec or close a trigger family.

## Committed facts, not status strings

The classifier will consume a typed transition only after its authoritative
mutation commits. Capture logical `task_id` or Swarm ID, current owning run,
process generation, current actual turn/need identity and causal event sequence.
Never use `exit_reason` substrings or model output to infer recovery exhaustion.
Record a recovery disposition explicitly (`pending`, `exhausted`, `unqualified`)
at the real decision boundary. Only `exhausted` makes a failure/loss/block eligible.
Unqualified routes remain inventory gaps, rather than become silent “verified”
negatives. No caller-supplied event, actor, preview text or pack key is authority.

The persistence proposal is a private semantic transition table, keyed by logical
subject and transition identity, with a small current-need table per subject.
Insert/update alongside the original producer's Store mutation; broadcast only
after commit through the existing emitted-event vectors. Use safe IDs/digests,
typed cause and lifecycle revision, never raw credentials/commands/native envelopes.
Do not add a public method capable of minting transitions. Any new public event
projection needs protocol-schema/generator checks before runtime qualification.
This table design requires source review before wiring; do not introduce a
parallel account/recovery authority or a second queue owner.

## Entry-point map

| Actual boundary | Change to make / silence obligation |
| --- | --- |
| `daemon.rs::tail_loop`, supervisor `shim.json` → running | Publish first actual work start after process observation. Existing `turn_started` precedes spawn and is not proof. Native `apply_norm::Running` can reach running first; cover it without a second start. Both paths use the same task-level start identity. |
| `daemon.rs::apply_norm::Permission` | After own-tool/session-grant automatic handling, atomically retain the live saved request and its first unresolved observation identity. Duplicate native request/status must retain identity; a request after the previous one cleared gets a new identity even if native request ID repeats. |
| `daemon.rs::answer_permission_always` / request claim and successful write | Clear only the claimed saved need identity, preserve failed-write rollback. Do not treat a claimed-but-unsent answer as resolved. Playback rereads current authority after arbiter wait. |
| `daemon.rs::apply_norm::Error` / `ErrorRetryAfter` | Preserve typed native error class as an input fact only. An error is not yet failed/cannot-continue/sign-in; later recovery/settlement decides. Duplicate error frames are not new needs. |
| `daemon.rs::finalize` | Compute typed cause from ExitInfo, actual TurnDone and interruption/budget markers, then run existing Continuity `park`. If parked, no terminal notification. Native auth/other failure requires an explicit permitted-recovery decision; false from `park` alone does not qualify every Auto/repair route. |
| `daemon.rs::mark_ended` | Update semantic settlement in the same savepoint as status/turn/workspace/claim settlement, using explicit caller cause/disposition. Successful status alone does not prove logical completion: inspect actual current queue/owner/validation/continuation state. Do not reread strings to invent a cause. |
| `daemon.rs::tail_loop` liveness loss | Live supervisor disappearance differs from never-started supervisor and startup reconciliation. Publish unexpected-loss candidate only after a real failed reattach/recovery decision; one identity, no second failure/generic cue. |
| `daemon.rs::reconcile` | Never create playable historical transitions for never-launched/lost/exited-while-down discoveries. Capture boot boundary before any live transition subscription; async tails of old exit records also retain startup provenance so they cannot become new live completion. |
| `handoff.rs::park` / retry / handoff | Pending recovery is silent. Successor uses predecessor task ID; owning run and generation change, first logical start identity does not. Retire predecessor needs only in the actual ownership transaction, preserve rollback. |
| `handoff.rs::give_up` | Its expiry mutation, failed settlement and subsequent actionable attention currently happen separately. Commit typed exhausted blocked cause plus live actionable need together; avoid an earlier generic failed cue. Memory/connection reasons are typed `wait.kind`, with actual retry/use-local actions. |
| `swarm/runtime.rs::launch_director` + owner binding/process start | Accepted start response can be blocked/uncertain; it is not readiness. Publish Swarm initiated only for committed current director generation and actual live ready execution. A duplicate/restart does not create a new logical Swarm start. |
| `swarm/completion.rs::complete` | Add completion candidate after the transaction validates workers, inbox, revision, integration and checks and commits `swarm_completions`. Current baseline completion assertion is UNREACHED; pinning Swarm start was the first failure. |
| `swarm/director.rs`, `swarm/owner.rs`, broker/admission result | Stalled/no-progress/lease/worker failure alone does not prove whole-Swarm recovery exhaustion. Publish whole owner need only at director-authoritative exhausted decision, with concrete action; coalesce worker/director reports under one Swarm identity. Unrepresented terminal decision remains a gap until supported. |
| Gate S proposal/conflict producers | Freeze actual proposal ID/revision/open or conflict ID/decision state; repeat open polling is the same need. Do not require a run ID for shared Overseer actionable needs, but do not let ordinary turns/watchers announce. |
| Future typed-pending producer | Native family/revision/generation/live lifecycle would be authority. Branch9040 is not integrated; no structured/browser/external-family coverage claim or silent dependency merge. |

## Every incumbent settlement caller

The following current callers must be explicitly accounted for when typed
settlement replaces generic status classification. Defaults must be unqualified,
never a guessed generic failure.

- create-task launch failure (`daemon.rs:1274`): not-started versus uncertain process; no started notification.
- delegated/pre-spawn failure (`1565/1582`), handoff-launch failure (`1762`), manual pool conflict (`1881`), start_turn refusal (`2151`): distinguish admission/rollback/recovery, do not claim logical task ended merely because attempted process did not launch.
- live tail loss (`2735`): actual spawned versus never-started, then recovery disposition.
- final process settlement (`3357`): actual ExitInfo/TurnDone/owner interrupt/budget plus recovery outcome.
- startup never-launched/lost (`3588/3619`): startup provenance, always silent for old transitions.
- failed successor launch rollback (`handoff.rs:451`): predecessor still owns work; no task failure announcement.
- exhausted wait (`handoff.rs:538`): typed blocked owner need; atomic with attention.
- Swarm-owned failed attempt (`handoff.rs:577`): subordinate, director owns recovery; silence.
- owner stop while parked (`handoff.rs:863`): deliberate Stop, silence.
- shared Overseer failed launch (`overseer/session.rs:510`): routine session silent; any genuine owner-actionable need has separate immutable proposal/need identity.

Line numbers are this exact base, not promises about later merged offsets.

## Runtime replacement and lock order

Producer holds existing launch/process gate as required, then Store; it releases
Store before broadcasting and all I/O. Subscriber receives only committed live
transition IDs, takes Store for a bounded current snapshot, and releases it before
coalescing, Voice arbitration or playback. No ownership-map/device/recovery gate
is acquired while holding Store. Existing Stop/handoff/queue gates remain owners
of those policies; audio must not compete with them.

Pending cue captures semantic identities and enablement epoch, not pack Selection.
Freshness snapshots happen immediately before arbitration and again afterward.
800ms collection counts distinct logical subjects, one per Swarm after suppression.
If2→1 use the surviving exact current need line;1→0 plays nothing. New need in
the same run can announce; restart/replay/repeated polling cannot resurrect old
needs. Queue capacities stay routine4/urgent2 with bounded overflow diagnostics.

Pack source preview calls proposed `audio::enqueue_preview(Line)->Result<()>`;
it returns a bounded queue/unavailable error rather than a false success. Runtime
worker owns Arc<Daemon> and calls `player::play(d,line,preview)` only after checks.
`enablement_changed(bool)` is disable-epoch/cancel/drain; source switching does
not discard current cues. Dedicated Heard feedback remains separate from Line.

## Next evidence gate

Seven semantic baseline failures are genuine at their first stated boundaries;
ordinary success/Stop controls passed. Preserve these receipts unchanged. Before
further runtime, source-review actual typed producers and identity/transaction
changes; then root allocates focused GREEN. Separately freeze gated freshness,
same-run-new-need, successor, replay and recovery outcomes; execute their intended
REDs before claiming those behaviors qualified. No per-line closure from nine cases.
