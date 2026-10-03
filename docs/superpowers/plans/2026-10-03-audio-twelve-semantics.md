# Audio Twelve Semantic Transitions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Play exactly the twelve owner-approved Audio Mode notifications for actual logical work and live owner needs, with truthful recovery, Swarm aggregation and playback freshness.

**Architecture:** Authoritative daemon transitions create typed semantic candidates; a daemon-owned subscriber coalesces and deduplicates their logical identities. A single bounded worker revalidates current semantic eligibility before and after the Voice arbiter, then asks the shared manifest player to resolve the current pack. Raw reason strings, model text and a bare completed/failed status never manufacture a more specific authority or recovery conclusion.

**Tech Stack:** Existing Rust daemon, Tokio broadcast/mpsc, SQLite Store, existing black-box Unix protocol test helpers and synthetic harness/Swarm fixtures. No new runtime dependency is proposed.

**Spec:** `docs/rfcs/audio-lines.md`, `docs/rfcs/audio-mode.md`, AC275–288 in `docs/overseer-rfc.md`, `docs/audits/2026-10-03-audio-twelve-trigger-inventory.md`.

## Global Constraints

- Exactly twelve canonical keys/phrases in the owner spec; no exposed legacy `agent_queued`, `agent_unblocked` or `agent_stopped` notifications.
- Distinct live top-level needs coalesce within 800 ms; routine lane capacity4, urgent lane capacity2; four start/completion keys routine, other eight urgent.
- Top-level logical task identity survives Continuity successors. Swarm replaces constituent director/worker notifications; watchers/shared Overseer routine turns are silent but actionable owner needs use the same classifier.
- Specific permission/reply/sign-in beats cannot-continue. Failure and unexpected loss are distinct terminal causes. Automatic retry/repair/reroute/fallback remains silent while available.
- Previous-daemon transitions never play. Resolve/supersede/disable cancels stale pending cues; check again after arbitration. Source switch resolves queued work against the current valid pack and may let an in-flight clip finish.
- Keep AC165 dedicated nonspoken Heard feedback, its latency/arbiter/no-self-capture behavior, and AC171/172 trusted confirmation/toast/Cancel/settle/exactly-once delivery. Conversational Voice answers remain speech.
- Private audio is read only by the pack slice when explicitly selected. This slice never reads POD or owner profiles, copies audio, runs providers or invokes runtime without a coordinator slot.
- Built-in asset content is pending the owner's earcon/spoken choice. Semantic keys and trigger scope remain twelve regardless; key logs alone do not prove spoken content or audition.
- Own branch based on79a334cd plus normal990d784a docs merge; never import unfinished typed-pending9040 or modify active integration/main. Criteria/ledger updates belong to coordinator.

## Ownership and Interface

Semantic slice creates `daemon/src/audio/lines.rs` with `Line::{ALL,parse,key,phrase,urgent}` and `daemon/src/audio/semantics.rs` for authoritative snapshot/identity/transition eligibility. It owns `audio.rs` subscriber/Cue/runtime/coalescing/freshness hunks and `voice/request.rs` legacy action cue removal plus dedicated Heard feedback with `voice/floor.rs` arbitration integration.

Pack slice owns new `audio/pack.rs`, `audio/source.rs`, `audio/player.rs`, audio.rs settings/play hunks and VSCode/TUI source controls. Agreed new internal interfaces: `source::snapshot(&Arc<Daemon>) -> Result<SourceSnapshot>`, `player::play(&Arc<Daemon>, Line, preview) -> Result<()>`, and idempotent `player::cancel(&Arc<Daemon>)`. Player resolves current source after semantic/arbiter waits. Semantic runtime exposes a disable-only enablement epoch/cancel+drain hook; source switching must not discard otherwise valid queued work. Synthetic captures remain `pack-id:canonical-key` after successful resolution/validation. Dedicated Heard feedback is not a `Line`, pack key or automatic Audio notification.

These are proposed interfaces, not claims that they already exist. Both agents author independent fixture checkpoints before sharing production hunks.

## Current Producers and Required Inventory

The checked-in source audit is the starting inventory. Its tables name raw events, authoritative state and gaps; update each row with actual fixture/evidence as qualified. The following minimum families are mandatory, not a substitute for additional supported producers:

| Line / AC | Real producer and authority | Required positive families | Required silence / present gaps |
| --- | --- | --- | --- |
| agent_started /275 | `daemon.rs` process-start status after actual spawn; first logical work identity | manual, admitted Auto, approved scheduled | pre-spawn `turn_started`, refused/queued, retry/reconnect/later turn, children; scheduled producer must be traced before claiming coverage |
| agent_complete /276 | `mark_ended` terminal success plus current continuation/queue/validation state | ordinary, Auto, recovered logical success | intermediate turn, queued successor, validation pending, failed/cancelled; no invented goal API: continuing per-agent/global-goal integration remains visible until its actual producer exists |
| agent_permission_required /277 | saved permission attention and owner-actionable proposal, after auto handling | command, file, network, browser/site, scoped native grant | duplicate permission/status, resolved/expired/superseded, sign-in; unfinished typed native path is a future integration gap |
| agent_reply_required /278 | saved question/form attention and explicit clarification state | plain, structured, multiple questions, form, clarification | permission/auth, answered/withdrawn/obsolete; bare waiting status is ambiguous and cannot mean reply |
| agent_sign_in_required /279 | structured auth error plus authorized recovery/fallback result | expired/missing login, provider/browser external auth, exhausted fallback | successful refresh/fallback, connectivity, approval, active refresh; installed/browser authorization unqualified |
| agent_cannot_continue /280 | `handoff::give_up`, exhausted admission/recovery result with actionable cause | capacity/quota/connectivity/environment/tool/input dependency | permitted recovery, ordinary queue/pause, solvable by agent; source must expose concrete action rather than generic failed text |
| agent_failed /281 | final settlement cause, logical task/successor recovery outcome | terminal harness/tool/validation/task failure, exhausted successor | command still repaired, recoverable turn, deliberate Stop |
| agent_stopped_unexpectedly /282 | actual supervisor loss + reattach/recovery outcome | crash, lost process/session/transport after failed restore | Stop/shutdown, short drop, restored execution, startup-discovered loss |
| agents_need_attention /283 | distinct top-level live need identities in window | mixed permission/reply/auth/blocked/failure | duplicate same need, children; plural must revalidate to single/zero |
| swarm_initiated /284 | accepted director owner + actual ready/running process | manual and Auto entry | start response alone, refused/queued, director reconnect, constituent starts |
| swarm_complete /285 | committed `swarm_completions` after checks/inbox/workers/integration validation | ordinary/recovered whole objective | worker/partial/unverified/failure/cancel; current complete transaction emits no audio transition |
| swarm_needs_attention /286 | authoritative whole-Swarm actionable need after director recovery exhausted | approval/clarification/auth/capacity/terminal block | recoverable worker trouble, duplicate director/worker reports; stalled alone does not establish exhausted recovery |

## Review Focus

1. A raw terminal status before recovery/continuation settles cannot prove logical completion or exhausted failure.
2. Same task with a new process generation must not replay start; same run with a genuinely new need must not remain permanently muted.
3. Swarm workers are often parentless `Run`s; use persisted director/worker membership rather than `parent_run_id` alone.
4. Model/public preview text cannot create permission/auth/recovery authority. Unknown native kinds remain unsupported/observable, not invented semantics.
5. Coalescing, lag/reconnect and Voice waits must preserve distinct identity and fresh current state; hold no Store/process lock while waiting/player I/O.

## Task 1 — Freeze Actual Boundary Baselines

Files: new `daemon/tests/audio_semantics.rs`; this plan; checked-in trigger inventory references.

- [x] Read exact owner spec and existing producers/helpers; isolate branch and agree shared ownership.
- [x] Author nine black-box tests using existing protocol/harness/Swarm entrypoints and independent canonical key literals. No new classifier helper or fabricated event-emission API is used.
- [ ] Coordinator grants runtime; package-only invalidate `overseerd` in exclusively owned retired target, build standalone daemon and no-run exact integration binary, retain fresh:false artifacts/.d source/hash proof.
- [ ] Run `nice -n20 env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 CARGO_TARGET_DIR=<allocated-retired-target> cargo test -p overseerd --test audio_semantics -- --nocapture` (nine named tests). Preserve actual feature RED versus setup/compile failure; ordinary success/owner Stop may correctly be baseline GREEN.
- [ ] Freeze source/logs and review reached assertions before production changes.

Initial exact tests: terminal failure, ordinary success, legacy permission+waiting dedup+owner resolution, expired auth with no permitted fallback, unrecoverable live supervisor loss, owner Stop silence, barrier-released two distinct permissions, accepted ready Swarm start with constituent suppression, and checked whole-Swarm completion. Existing selected-player test sink establishes incumbent boundary only; after pack dependency qualifies, select synthetic twelve-file packs and retain selected-file captures to satisfy event-to-resolver proof. This nine-test batch is not full AC275–288 verification.

## Task 2 — Authoritative Transitions and Logical Identity

Files: new `audio/lines.rs`, `audio/semantics.rs`; focused producer changes in `daemon.rs`, `handoff.rs`, `store.rs`, `swarm/runtime.rs`, `swarm/completion.rs`, `swarm/director.rs`, `swarm/owner.rs`, and `server_swarm*.rs` only where audit establishes a real committed transition.

- [ ] Add actual boundaries for first-start-before/after-spawn, refused/queued/Auto/scheduled, intermediate/final/queued continuation, repair/exhaustion, lost/restore and whole-Swarm need/verification. Tests must first prove missing behavior; native/external families lacking a supported producer stay explicit gaps, not mock qualification.
- [ ] Define immutable typed candidate identity: logical subject (`Task(task_id)` or `Swarm(run_id)`), semantic kind, authoritative lifecycle revision/need identity, generation when freshness requires it, and causal event sequence. Existing `Store` task/run IDs and saved permission request ID are authority; no caller-provided actor or arbitrary text selector.
- [ ] Publish candidates only after authoritative transaction commit. Preserve current status/events/public semantics; introduce typed cause metadata where existing status is insufficient. Do not infer auth versus failure from a substring.
- [ ] Started dedup uses logical task first actual work, not process-generation or `turn.n==1`; successful task completion requires no known queued/continuing/recovery/validation work. Unknown continuation support must not be described as verified.
- [ ] Specific live needs outrank generic blocking; terminal failed/lost causes have one transition, no second generic cue. Snapshot must expose actionable reason from saved authority; it must not expose private raw native envelope.
- [ ] Identify Swarm membership from `swarm_worker_launches`/`swarm_director_owners`; whole objective completion from validated `swarm_completions`, not worker exit.
- [ ] Preserve exact logical transition/need dedup across repeated live publications; old event sequences and daemon restart never enqueue historical candidates. Do not arbitrarily evict active need identities from a run-only HashSet. Review finite retention/cleanup before introducing any new durable receipt table.
- [ ] Run only allocated focused tests, obtain source review, commit.

## Task 3 — Coalescing, Playback Freshness and Cancellation

Files: `audio.rs` runtime/subscriber; `audio/semantics.rs`; new fixture-only gated arbitration observations, integration tests in `audio_semantics.rs`; pack interfaces as agreed.

- [ ] First author deterministic hold fixtures for resolve/supersede before dequeue and during arbiter wait, duplicate replay, same-run second need, mixed distinct needs, window resolution to one/zero, lag/reconnect and restart live-only behavior. A bounded fixture gate observes actual stages and releases without Store/process locks; it cannot inject authority.
- [ ] Replace same-key BurstGate with 800ms candidate collection keyed by logical need. Distinct top-level subjects produce plural; Swarm counts once after constituent aggregation. At delivery choose plural for2+, exact current specific line for1, silence for0.
- [ ] Cue retains identity/enablement epoch, not a captured pack selection. Revalidate immediately before arbitration and again afterward; source resolves only at play. If arbiter wait changes cardinality, reselect line and proper lane without duplicate playback.
- [ ] Keep four routine/eight urgent keys and bounded4/2 queues. Preserve nonblocking producers, finite coalescing memory and one player. Define overflow as bounded logged/drop policy, never silent unbounded allocation.
- [ ] Enablement epoch changes on disable; cancel player and drain queued cues. Source change does not bump epoch. Preview explicitly works while disabled via same player but never creates a semantic transition/dedup receipt.
- [ ] Test two/no clients, disable during held/playing/queued work, source switch during held cue and in-flight clip, shutdown owned cleanup. No blocking locks across Voice/player waits.
- [ ] Run allocated tests, independently review privacy/lock order/races, commit.

## Task 4 — Voice Feedback and Legacy Cue Removal

Files: `voice/request.rs`, `voice/floor.rs`, affected `daemon/tests/voice.rs`, current `audio.rs` callers/tests.

- [ ] Freeze actual trusted Allow/Deny confirmation/toast/Cancel/settle controls with capture proving neither legacy cue; do not weaken exactly-once permission bytes.
- [ ] Replace heard_signal's `audio::cue("agent_queued")` with a dedicated nonspoken feedback operation under the same speaker/mic arbitration. It is separate from the12 line/manifest contract and does not expose another automatic Audio key.
- [ ] Update arbiter urgent classification using canonical Line rather than old `agent_needs_attention` string; preserve owner/Overseer conversational behavior and AC165 latency/no-self-capture evidence.
- [ ] Maintain explicit compatibility controls for conversational replies; inspect every legacy cue call site and migration alias. Runtime aliases cannot cause automatic legacy notifications.
- [ ] Execute allocated simulated fixtures first; native speech/owner audition needs its own qualification and cannot be inferred from logs.

## Task 5 — Pack Integration and Clause Qualification

- [ ] Normally merge reviewed pack slice only after both agents freeze shared hunks; regenerate protocol artifacts for any actual changed public events, no manual invented generated output.
- [ ] Re-run every qualified producer using synthetic distinguishable twelve-file Built-in/external packs through common resolver. Update inventory with exact raw event/state, recovery result, logical identity, key/silence, test and capture paths.
- [ ] Account for every AC275–286 minimum family plus duplicate/resolve/supersede/restart per line. Unsupported browser/native/scheduled/goal paths remain named gaps until real producer/runtime proof; do not declare per-line complete from these first nine tests.
- [ ] Coordinate full test-all, packaged UI/TUI, Voice overlap and actual built-in content/audition gates. No separate runtime while full lock active. Owner private audition produces metadata only, never uploaded private clips.
- [ ] Push implementation/evidence branch at bounded checkpoints; leave draft and ledger partial until literal Verify clauses and integration gate pass. Coordinator updates main records with explicit commits.

## Current Checkpoint

Nine fixtures authored against unchanged runtime at base d1f2ea88. No Cargo, test body, helper, daemon, speech, UI or provider ran. No RED/GREEN claim exists. Production changes await actual baseline and source-design review. Built-in content choice and unfinished native pending integration remain explicit external dependencies.
